/// Returns (input_cents_per_mtok, output_cents_per_mtok)
pub fn price_per_mtok(model: &str) -> (u64, u64) {
    price_per_mtok_with_provider("anthropic", model)
}

/// Provider-aware pricing table. Prices are in cents per 1M tokens.
pub fn price_per_mtok_with_provider(provider: &str, model: &str) -> (u64, u64) {
    let m = model;
    match provider {
        "ollama" => (0, 0), // local = free, still tracked
        "openai" => match () {
            _ if m.starts_with("gpt-4o-mini") => (15, 60),
            _ if m.starts_with("gpt-4o") => (250, 1000),
            _ if m.starts_with("gpt-4.1") => (200, 800),
            _ if m.starts_with("gpt-4") => (250, 1000),
            _ if m.starts_with("o1") => (1500, 6000),
            _ if m.starts_with("o3") => (1000, 4000),
            _ if m.starts_with("o4-mini") => (110, 440),
            _ if m.starts_with("o4") => (300, 1200),
            _ => (250, 1000), // default OpenAI fallback
        },
        // anthropic or unknown — legacy path
        _ => match () {
            _ if m.contains("opus") => (500, 2500),
            _ if m.contains("sonnet") => (300, 1500),
            _ if m.contains("haiku") => (80, 400),
            _ => (300, 1500),
        },
    }
}

/// Calculate cost in cents from token counts (legacy: assumes anthropic pricing).
pub fn calculate_cost(model: &str, input_tokens: i32, output_tokens: i32) -> i64 {
    calculate_cost_with_provider("anthropic", model, input_tokens, output_tokens)
}

/// Calculate cost in cents from token counts with provider-aware pricing.
///
/// Uses ceiling division so any non-zero usage rounds up to at least 1 cent —
/// small requests on cheap models would otherwise vanish to zero and never show
/// up in spend logs or budget checks. Providers with zero pricing (Ollama) stay
/// at 0 regardless.
pub fn calculate_cost_with_provider(
    provider: &str,
    model: &str,
    input_tokens: i32,
    output_tokens: i32,
) -> i64 {
    let (in_price, out_price) = price_per_mtok_with_provider(provider, model);
    let input_tokens = input_tokens.max(0) as u64;
    let output_tokens = output_tokens.max(0) as u64;
    let numerator = input_tokens * in_price + output_tokens * out_price;
    if numerator == 0 {
        0
    } else {
        numerator.div_ceil(1_000_000) as i64
    }
}

/// Extract model and usage from a non-streaming Anthropic JSON response body.
pub fn extract_usage_from_json(body: &[u8]) -> Option<(String, i32, i32)> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let model = v.get("model")?.as_str()?.to_string();
    let usage = v.get("usage")?;
    let input = usage.get("input_tokens")?.as_i64()? as i32;
    let output = usage.get("output_tokens")?.as_i64()? as i32;
    Some((model, input, output))
}

/// Extract model and usage from an OpenAI `/v1/chat/completions` response.
pub fn extract_usage_from_openai_json(body: &[u8]) -> Option<(String, i32, i32)> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let model = v.get("model")?.as_str()?.to_string();
    let usage = v.get("usage")?;
    let input = usage.get("prompt_tokens")?.as_i64()? as i32;
    let output = usage.get("completion_tokens")?.as_i64()? as i32;
    Some((model, input, output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_haiku_request_rounds_up_to_one_cent() {
        // 27 * 80 + 7 * 400 = 4960 micro-cents = 0.00496 cents → ceil to 1
        assert_eq!(calculate_cost("claude-haiku-4-5", 27, 7), 1);
    }

    #[test]
    fn zero_tokens_cost_zero() {
        assert_eq!(calculate_cost("claude-sonnet-4-5", 0, 0), 0);
    }

    #[test]
    fn single_input_token_rounds_up() {
        // Even a single input token on haiku should bill at least 1 cent.
        assert_eq!(calculate_cost("claude-haiku-4-5", 1, 0), 1);
    }

    #[test]
    fn exact_one_cent_on_haiku() {
        // 12_500 input tokens * 80 cents/Mtok = 1 cent exactly.
        assert_eq!(calculate_cost("claude-haiku-4-5", 12_500, 0), 1);
    }

    #[test]
    fn just_over_one_cent_rolls_to_two() {
        // 12_501 input tokens = 1_000_080 micro-cents → ceil to 2 cents.
        assert_eq!(calculate_cost("claude-haiku-4-5", 12_501, 0), 2);
    }

    #[test]
    fn large_request_matches_expected_cents() {
        // Sonnet: 1M input = 300 cents, 1M output = 1500 cents → total 1800.
        assert_eq!(
            calculate_cost("claude-sonnet-4-5", 1_000_000, 1_000_000),
            1800
        );
    }

    #[test]
    fn opus_pricing_applied() {
        // Opus: 1M input = 500 cents, 1M output = 2500 cents → 3000.
        assert_eq!(
            calculate_cost("claude-opus-4-5", 1_000_000, 1_000_000),
            3000
        );
    }

    #[test]
    fn ollama_provider_is_free() {
        assert_eq!(
            calculate_cost_with_provider("ollama", "qwen2.5:3b", 1000, 500),
            0
        );
    }

    #[test]
    fn openai_gpt4o_mini_pricing() {
        // gpt-4o-mini: 15/60 cents per Mtok. 10k in + 5k out = 150 + 300 = 450 nano → ceil to 1.
        assert_eq!(
            calculate_cost_with_provider("openai", "gpt-4o-mini", 10_000, 5_000),
            1
        );
    }

    #[test]
    fn extracts_usage_from_openai_response() {
        let body = br#"{"model":"gpt-4o","usage":{"prompt_tokens":42,"completion_tokens":7}}"#;
        let (m, i, o) = extract_usage_from_openai_json(body).unwrap();
        assert_eq!(m, "gpt-4o");
        assert_eq!(i, 42);
        assert_eq!(o, 7);
    }
}
