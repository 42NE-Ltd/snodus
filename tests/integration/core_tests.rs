// Integration tests for snodus-core pure-logic functions.
// No database, no network, no async runtime required.

use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use serde_json::json;
use uuid::Uuid;

// ─── spend/counter ─────────────────────────────────────────────────────────────

use snodus_core::spend::counter;

#[test]
fn cost_opus_1m_tokens() {
    // Opus: 500 cents/Mtok input, 2500 cents/Mtok output → 3000 total
    assert_eq!(
        counter::calculate_cost("claude-opus-4-5", 1_000_000, 1_000_000),
        3000
    );
}

#[test]
fn cost_sonnet_1m_tokens() {
    // Sonnet: 300 + 1500 → 1800
    assert_eq!(
        counter::calculate_cost("claude-sonnet-4-5", 1_000_000, 1_000_000),
        1800
    );
}

#[test]
fn cost_haiku_small_rounds_up() {
    // Small haiku request always rounds up to at least 1 cent
    assert_eq!(counter::calculate_cost("claude-haiku-4-5", 1, 0), 1);
}

#[test]
fn cost_zero_tokens_is_zero() {
    assert_eq!(counter::calculate_cost("claude-sonnet-4-5", 0, 0), 0);
}

#[test]
fn cost_negative_tokens_clamped_to_zero() {
    // Negative tokens should not produce negative cost
    assert_eq!(counter::calculate_cost("claude-sonnet-4-5", -100, -50), 0);
}

#[test]
fn cost_with_provider_openai_gpt4o_mini() {
    // gpt-4o-mini: 15/60 cents per Mtok
    // 1M input + 1M output = 15 + 60 = 75 cents
    assert_eq!(
        counter::calculate_cost_with_provider("openai", "gpt-4o-mini", 1_000_000, 1_000_000),
        75
    );
}

#[test]
fn cost_with_provider_openai_gpt4o() {
    // gpt-4o: 250/1000 cents per Mtok → 1250
    assert_eq!(
        counter::calculate_cost_with_provider("openai", "gpt-4o", 1_000_000, 1_000_000),
        1250
    );
}

#[test]
fn cost_with_provider_ollama_is_free() {
    assert_eq!(
        counter::calculate_cost_with_provider("ollama", "llama3.2:1b", 500_000, 200_000),
        0
    );
}

#[test]
fn cost_with_provider_ollama_zero_even_with_large_usage() {
    assert_eq!(
        counter::calculate_cost_with_provider("ollama", "qwen2.5:3b", 10_000_000, 5_000_000),
        0
    );
}

#[test]
fn price_per_mtok_anthropic_opus() {
    assert_eq!(counter::price_per_mtok("claude-opus-4-5"), (500, 2500));
}

#[test]
fn price_per_mtok_anthropic_sonnet() {
    assert_eq!(counter::price_per_mtok("claude-sonnet-4-5"), (300, 1500));
}

#[test]
fn price_per_mtok_anthropic_haiku() {
    assert_eq!(counter::price_per_mtok("claude-haiku-4-5"), (80, 400));
}

#[test]
fn price_per_mtok_anthropic_unknown_defaults_to_sonnet() {
    assert_eq!(counter::price_per_mtok("claude-future-model"), (300, 1500));
}

#[test]
fn price_per_mtok_with_provider_openai_o1() {
    assert_eq!(
        counter::price_per_mtok_with_provider("openai", "o1-preview"),
        (1500, 6000)
    );
}

#[test]
fn price_per_mtok_with_provider_openai_o3() {
    assert_eq!(
        counter::price_per_mtok_with_provider("openai", "o3-mini"),
        (1000, 4000)
    );
}

#[test]
fn extract_usage_from_anthropic_json() {
    let body = br#"{"model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":50}}"#;
    let (model, input, output) = counter::extract_usage_from_json(body).unwrap();
    assert_eq!(model, "claude-sonnet-4-5");
    assert_eq!(input, 100);
    assert_eq!(output, 50);
}

#[test]
fn extract_usage_from_anthropic_json_missing_usage_returns_none() {
    let body = br#"{"model":"claude-sonnet-4-5"}"#;
    assert!(counter::extract_usage_from_json(body).is_none());
}

#[test]
fn extract_usage_from_openai_json_valid() {
    let body = br#"{"model":"gpt-4o","usage":{"prompt_tokens":42,"completion_tokens":7}}"#;
    let (model, input, output) = counter::extract_usage_from_openai_json(body).unwrap();
    assert_eq!(model, "gpt-4o");
    assert_eq!(input, 42);
    assert_eq!(output, 7);
}

#[test]
fn extract_usage_from_openai_json_invalid_returns_none() {
    let body = b"not json at all";
    assert!(counter::extract_usage_from_openai_json(body).is_none());
}

// ─── providers/router ──────────────────────────────────────────────────────────

use snodus_core::providers::router::RuleRouter;

#[test]
fn router_short_input_routes_to_haiku() {
    let body = json!({"messages": [{"role": "user", "content": "hi there"}]});
    let d = RuleRouter::default().route(&body);
    assert!(
        d.target_model.contains("haiku"),
        "expected haiku, got {}",
        d.target_model
    );
}

#[test]
fn router_medium_input_routes_to_sonnet() {
    // 1000 chars / 4 = 250 tokens, above short_threshold(200), below long_threshold(2000)
    let text = "x".repeat(1000);
    let body = json!({"messages": [{"role": "user", "content": text}]});
    let d = RuleRouter::default().route(&body);
    assert!(
        d.target_model.contains("sonnet"),
        "expected sonnet, got {}",
        d.target_model
    );
}

#[test]
fn router_long_input_routes_to_opus() {
    // 12000 chars / 4 = 3000 tokens, above long_threshold(2000)
    let text = "x".repeat(12000);
    let body = json!({"messages": [{"role": "user", "content": text}]});
    let d = RuleRouter::default().route(&body);
    assert!(
        d.target_model.contains("opus"),
        "expected opus, got {}",
        d.target_model
    );
}

#[test]
fn router_keyword_refactor_routes_to_opus() {
    let body = json!({"messages": [{"role": "user", "content": "please refactor this function"}]});
    let d = RuleRouter::default().route(&body);
    assert!(d.target_model.contains("opus"));
    assert_eq!(d.reason, "keyword:complex");
}

#[test]
fn router_keyword_translate_routes_to_haiku() {
    let body =
        json!({"messages": [{"role": "user", "content": "translate this to French please"}]});
    let d = RuleRouter::default().route(&body);
    assert!(d.target_model.contains("haiku"));
    assert_eq!(d.reason, "keyword:simple");
}

#[test]
fn router_empty_body_routes_to_haiku() {
    let body = json!({"messages": []});
    let d = RuleRouter::default().route(&body);
    assert!(
        d.target_model.contains("haiku"),
        "empty input should be short → haiku"
    );
}

#[test]
fn router_system_prompt_counted_in_tokens() {
    // System text of 9000 chars + message of 100 chars = 9100 / 4 = 2275 tokens → opus
    let system = "y".repeat(9000);
    let body = json!({
        "system": system,
        "messages": [{"role": "user", "content": "hello"}]
    });
    let d = RuleRouter::default().route(&body);
    assert!(
        d.target_model.contains("opus"),
        "system text should push to opus"
    );
}

#[test]
fn router_keyword_overrides_size() {
    // Even though the input is short, "refactor" keyword should force opus
    let body = json!({"messages": [{"role": "user", "content": "refactor"}]});
    let d = RuleRouter::default().route(&body);
    assert!(d.target_model.contains("opus"));
    assert_eq!(d.reason, "keyword:complex");
}

// ─── providers/pool ────────────────────────────────────────────────────────────

use snodus_core::providers::anthropic::AnthropicProvider;
use snodus_core::providers::ollama::OllamaProvider;
use snodus_core::providers::openai::OpenAiProvider;
use snodus_core::providers::pool::ProviderPool;
use snodus_core::providers::ProviderError;

#[test]
fn pool_resolves_claude_to_anthropic() {
    let mut pool = ProviderPool::new();
    pool.register(Arc::new(AnthropicProvider::new("sk-ant-fake".into())));
    pool.register(Arc::new(OpenAiProvider::new("sk-openai-fake".into())));

    let p = pool.resolve("claude-sonnet-4-5").unwrap();
    assert_eq!(p.name(), "anthropic");
}

#[test]
fn pool_resolves_gpt_to_openai() {
    let mut pool = ProviderPool::new();
    pool.register(Arc::new(AnthropicProvider::new("sk-ant-fake".into())));
    pool.register(Arc::new(OpenAiProvider::new("sk-openai-fake".into())));

    let p = pool.resolve("gpt-4o-mini").unwrap();
    assert_eq!(p.name(), "openai");
}

#[test]
fn pool_resolves_llama_to_ollama() {
    let mut pool = ProviderPool::new();
    pool.register(Arc::new(OllamaProvider::new(
        "http://localhost:11434".into(),
    )));

    let p = pool.resolve("llama3.2").unwrap();
    assert_eq!(p.name(), "ollama");
}

#[test]
fn pool_unknown_model_returns_error() {
    let mut pool = ProviderPool::new();
    pool.register(Arc::new(AnthropicProvider::new("sk-ant-fake".into())));

    match pool.resolve("unknown-model") {
        Err(ProviderError::ModelNotSupported(m)) => {
            assert_eq!(m, "unknown-model");
        }
        Ok(p) => panic!("expected ModelNotSupported, got provider: {}", p.name()),
        Err(e) => panic!("expected ModelNotSupported, got error: {}", e),
    }
}

#[test]
fn pool_disabled_provider_skipped() {
    let mut pool = ProviderPool::new();
    // Empty API key → is_enabled() returns false
    pool.register(Arc::new(AnthropicProvider::new(String::new())));

    match pool.resolve("claude-sonnet-4-5") {
        Err(ProviderError::ModelNotSupported(_)) => {}
        Ok(p) => panic!(
            "expected ModelNotSupported for disabled provider, got provider: {}",
            p.name()
        ),
        Err(e) => panic!(
            "expected ModelNotSupported for disabled provider, got error: {}",
            e
        ),
    }
}

#[test]
fn pool_empty_pool_returns_error() {
    let pool = ProviderPool::new();
    assert!(pool.resolve("anything").is_err());
    assert!(pool.is_empty());
}

// ─── providers/translate ───────────────────────────────────────────────────────

use snodus_core::providers::translate::anthropic_to_openai;
use snodus_core::providers::translate::openai_to_anthropic;

#[test]
fn translate_anthropic_to_openai_system_becomes_message() {
    let anth = json!({
        "model": "gpt-4o",
        "max_tokens": 100,
        "system": "You are a helpful assistant.",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let out = anthropic_to_openai::translate_request(&anth).unwrap();
    let msgs = out["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "You are a helpful assistant.");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[1]["content"], "Hello");
}

#[test]
fn translate_anthropic_to_openai_content_parts_flattened() {
    let anth = json!({
        "model": "gpt-4o",
        "max_tokens": 10,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "part one"},
                {"type": "text", "text": " part two"}
            ]
        }]
    });
    let out = anthropic_to_openai::translate_request(&anth).unwrap();
    assert_eq!(out["messages"][0]["content"], "part one part two");
}

#[test]
fn translate_anthropic_to_openai_max_completion_tokens_for_o1() {
    let anth = json!({
        "model": "o1-preview",
        "max_tokens": 200,
        "messages": [{"role": "user", "content": "think hard"}]
    });
    let out = anthropic_to_openai::translate_request(&anth).unwrap();
    assert_eq!(out["max_completion_tokens"], 200);
    assert!(out.get("max_tokens").is_none());
}

#[test]
fn translate_anthropic_to_openai_max_completion_tokens_for_o3() {
    let anth = json!({
        "model": "o3-mini",
        "max_tokens": 500,
        "messages": [{"role": "user", "content": "reason"}]
    });
    let out = anthropic_to_openai::translate_request(&anth).unwrap();
    assert_eq!(out["max_completion_tokens"], 500);
    assert!(out.get("max_tokens").is_none());
}

#[test]
fn translate_anthropic_to_openai_regular_model_uses_max_tokens() {
    let anth = json!({
        "model": "gpt-4o-mini",
        "max_tokens": 64,
        "messages": [{"role": "user", "content": "hi"}]
    });
    let out = anthropic_to_openai::translate_request(&anth).unwrap();
    assert_eq!(out["max_tokens"], 64);
    assert!(out.get("max_completion_tokens").is_none());
}

#[test]
fn translate_openai_to_anthropic_basic_response() {
    let openai = json!({
        "id": "chatcmpl-abc",
        "model": "gpt-4o-mini",
        "choices": [{
            "message": {"role": "assistant", "content": "Hello!"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 3}
    });
    let out = openai_to_anthropic::translate_response(&openai).unwrap();
    assert_eq!(out["model"], "gpt-4o-mini");
    assert_eq!(out["content"][0]["type"], "text");
    assert_eq!(out["content"][0]["text"], "Hello!");
    assert_eq!(out["stop_reason"], "end_turn");
    assert_eq!(out["usage"]["input_tokens"], 10);
    assert_eq!(out["usage"]["output_tokens"], 3);
}

#[test]
fn translate_openai_to_anthropic_length_maps_to_max_tokens() {
    let openai = json!({
        "model": "gpt-4o",
        "choices": [{"message": {"content": "truncated"}, "finish_reason": "length"}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 100}
    });
    let out = openai_to_anthropic::translate_response(&openai).unwrap();
    assert_eq!(out["stop_reason"], "max_tokens");
}

#[test]
fn translate_openai_to_anthropic_tool_calls_maps_to_tool_use() {
    let openai = json!({
        "model": "gpt-4o",
        "choices": [{"message": {"content": ""}, "finish_reason": "tool_calls"}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 10}
    });
    let out = openai_to_anthropic::translate_response(&openai).unwrap();
    assert_eq!(out["stop_reason"], "tool_use");
}

#[test]
fn translate_openai_to_anthropic_missing_choices_returns_error() {
    let openai = json!({"model": "gpt-4o", "usage": {}});
    assert!(openai_to_anthropic::translate_response(&openai).is_err());
}

// ─── budget/mod ────────────────────────────────────────────────────────────────

use snodus_core::budget::BudgetChecker;
use snodus_core::budget::BudgetResult;

#[test]
fn budget_allowed_with_remaining() {
    match BudgetChecker::judge(1000, 200) {
        BudgetResult::Allowed {
            remaining_cents,
            spent_cents,
            limit_cents,
        } => {
            assert_eq!(remaining_cents, 800);
            assert_eq!(spent_cents, 200);
            assert_eq!(limit_cents, 1000);
        }
        other => panic!("expected Allowed, got {:?}", other),
    }
}

#[test]
fn budget_denied_when_at_limit() {
    match BudgetChecker::judge(1000, 1000) {
        BudgetResult::Denied {
            limit_cents,
            spent_cents,
        } => {
            assert_eq!(limit_cents, 1000);
            assert_eq!(spent_cents, 1000);
        }
        other => panic!("expected Denied, got {:?}", other),
    }
}

#[test]
fn budget_denied_when_over_limit() {
    assert!(matches!(
        BudgetChecker::judge(1000, 1500),
        BudgetResult::Denied { .. }
    ));
}

#[test]
fn budget_zero_limit_means_unlimited() {
    assert!(matches!(
        BudgetChecker::judge(0, 0),
        BudgetResult::Unlimited
    ));
    assert!(matches!(
        BudgetChecker::judge(0, 999_999),
        BudgetResult::Unlimited
    ));
}

#[test]
fn budget_negative_limit_means_unlimited() {
    assert!(matches!(
        BudgetChecker::judge(-1, 500),
        BudgetResult::Unlimited
    ));
    assert!(matches!(
        BudgetChecker::judge(-100, 0),
        BudgetResult::Unlimited
    ));
}

#[test]
fn budget_allowed_remaining_one_cent() {
    match BudgetChecker::judge(1000, 999) {
        BudgetResult::Allowed {
            remaining_cents, ..
        } => {
            assert_eq!(remaining_cents, 1);
        }
        other => panic!("expected Allowed with 1 remaining, got {:?}", other),
    }
}

// ─── ratelimit/mod ─────────────────────────────────────────────────────────────

use snodus_core::ratelimit::{RateLimitResult, RateLimiter};

#[test]
fn ratelimit_allows_up_to_limit() {
    let rl = RateLimiter::new(60);
    let key = Uuid::new_v4();

    for i in 0..5 {
        match rl.check(key, 5) {
            RateLimitResult::Allowed {
                remaining, limit, ..
            } => {
                assert_eq!(limit, 5);
                assert_eq!(remaining, 4 - i);
            }
            _ => panic!("expected Allowed on call {}", i + 1),
        }
    }
}

#[test]
fn ratelimit_denies_over_limit() {
    let rl = RateLimiter::new(60);
    let key = Uuid::new_v4();

    for _ in 0..5 {
        rl.check(key, 5);
    }

    match rl.check(key, 5) {
        RateLimitResult::Denied {
            limit,
            retry_after_secs,
        } => {
            assert_eq!(limit, 5);
            assert!(retry_after_secs > 0);
        }
        _ => panic!("expected Denied on 6th call"),
    }
}

#[test]
fn ratelimit_resets_after_window() {
    let rl = RateLimiter::new(1); // 1 second window
    let key = Uuid::new_v4();

    for _ in 0..3 {
        rl.check(key, 3);
    }
    assert!(matches!(rl.check(key, 3), RateLimitResult::Denied { .. }));

    sleep(Duration::from_millis(1100));

    assert!(matches!(rl.check(key, 3), RateLimitResult::Allowed { .. }));
}

#[test]
fn ratelimit_different_keys_isolated() {
    let rl = RateLimiter::new(60);
    let k1 = Uuid::new_v4();
    let k2 = Uuid::new_v4();

    for _ in 0..3 {
        rl.check(k1, 3);
    }

    assert!(matches!(rl.check(k1, 3), RateLimitResult::Denied { .. }));
    assert!(matches!(rl.check(k2, 3), RateLimitResult::Allowed { .. }));
}

#[test]
fn ratelimit_remaining_decreases() {
    let rl = RateLimiter::new(60);
    let key = Uuid::new_v4();

    let r1 = rl.check(key, 10);
    let r2 = rl.check(key, 10);
    let r3 = rl.check(key, 10);

    match (r1, r2, r3) {
        (
            RateLimitResult::Allowed { remaining: r1, .. },
            RateLimitResult::Allowed { remaining: r2, .. },
            RateLimitResult::Allowed { remaining: r3, .. },
        ) => {
            assert_eq!(r1, 9);
            assert_eq!(r2, 8);
            assert_eq!(r3, 7);
        }
        _ => panic!("expected all Allowed"),
    }
}

// ─── auth/mod (key hashing) ───────────────────────────────────────────────────

use snodus_core::db::keys::hash_key;

#[test]
fn hash_key_consistent() {
    let h1 = hash_key("sk-test123");
    let h2 = hash_key("sk-test123");
    assert_eq!(h1, h2, "hashing the same key must produce the same hash");
}

#[test]
fn hash_key_is_sha256_hex() {
    let h = hash_key("sk-test123");
    // SHA-256 hex is always 64 characters
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_key_different_inputs_different_hashes() {
    let h1 = hash_key("sk-aaa");
    let h2 = hash_key("sk-bbb");
    assert_ne!(h1, h2);
}

#[test]
fn hash_key_known_value() {
    // Pre-computed SHA-256 of "sk-test123"
    // $ echo -n "sk-test123" | shasum -a 256
    let expected = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"sk-test123");
        hex::encode(hasher.finalize())
    };
    assert_eq!(hash_key("sk-test123"), expected);
}

// ─── providers/sse ─────────────────────────────────────────────────────────────

// The SSE parsing functions are not pub — they are crate-private. We test the
// module through the publicly observable effects. Since `process_event` and
// `parse_events_from_buffer` are `pub(crate)`, we test via the `intercept`
// function which is pub. However `intercept` requires an async runtime. Instead,
// we re-test the observable contract: the SseUsage struct is public and the
// intercept function's behaviour is verified via its callback.
//
// Since the internal functions are private, we rely on the inline unit tests
// already in sse.rs and test the public SseUsage struct here.

use snodus_core::providers::sse::SseUsage;

#[test]
fn sse_usage_default_is_zero() {
    let u = SseUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert!(u.model.is_none());
}

#[test]
fn sse_usage_fields_can_be_set() {
    let u = SseUsage {
        input_tokens: 15,
        output_tokens: 142,
        model: Some("claude-sonnet-4-5".into()),
    };
    assert_eq!(u.input_tokens, 15);
    assert_eq!(u.output_tokens, 142);
    assert_eq!(u.model.as_deref(), Some("claude-sonnet-4-5"));
}

// ─── providers/ollama (translation functions) ──────────────────────────────────

use snodus_core::providers::ollama::{translate_request_to_ollama, translate_response_from_ollama};

#[test]
fn ollama_translate_request_with_system() {
    let anth = json!({
        "model": "qwen2.5:3b",
        "max_tokens": 50,
        "system": "be helpful",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let out = translate_request_to_ollama(&anth).unwrap();
    assert_eq!(out["model"], "qwen2.5:3b");
    let msgs = out["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "be helpful");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[1]["content"], "hi");
    assert_eq!(out["stream"], false);
    assert_eq!(out["options"]["num_predict"], 50);
}

#[test]
fn ollama_translate_response_to_anthropic_shape() {
    let raw = json!({
        "model": "qwen2.5:3b",
        "message": {"role": "assistant", "content": "hello world"},
        "prompt_eval_count": 12,
        "eval_count": 4
    });
    let out = translate_response_from_ollama(&raw);
    assert_eq!(out["role"], "assistant");
    assert_eq!(out["content"][0]["type"], "text");
    assert_eq!(out["content"][0]["text"], "hello world");
    assert_eq!(out["usage"]["input_tokens"], 12);
    assert_eq!(out["usage"]["output_tokens"], 4);
    assert_eq!(out["stop_reason"], "end_turn");
}

#[test]
fn ollama_handles_model_static_prefix() {
    let p = OllamaProvider::new("http://localhost:11434".into());
    assert!(p.handles_model("qwen2.5:3b"));
    assert!(p.handles_model("llama3.2:1b"));
    assert!(p.handles_model("mistral:7b"));
    assert!(p.handles_model("deepseek-coder:6.7b"));
    assert!(!p.handles_model("claude-sonnet-4-5"));
    assert!(!p.handles_model("gpt-4o"));
}

// ─── cross-cutting: provider trait basics ──────────────────────────────────────

use snodus_core::providers::{Provider, RequestFormat};

#[test]
fn anthropic_provider_native_format_is_anthropic() {
    let p = AnthropicProvider::new("key".into());
    assert!(matches!(p.native_format(), RequestFormat::Anthropic));
}

#[test]
fn openai_provider_native_format_is_openai() {
    let p = OpenAiProvider::new("key".into());
    assert!(matches!(p.native_format(), RequestFormat::OpenAi));
}

#[test]
fn anthropic_provider_handles_claude_models() {
    let p = AnthropicProvider::new("key".into());
    assert!(p.handles_model("claude-sonnet-4-5"));
    assert!(p.handles_model("claude-opus-4-5"));
    assert!(p.handles_model("claude-haiku-4-5"));
    assert!(!p.handles_model("gpt-4o"));
}

#[test]
fn openai_provider_handles_gpt_and_o_models() {
    let p = OpenAiProvider::new("key".into());
    assert!(p.handles_model("gpt-4o-mini"));
    assert!(p.handles_model("gpt-4o"));
    assert!(p.handles_model("o1-preview"));
    assert!(p.handles_model("o3-mini"));
    assert!(!p.handles_model("claude-sonnet-4-5"));
}

#[test]
fn provider_enabled_with_key_disabled_without() {
    let enabled = AnthropicProvider::new("sk-ant-real".into());
    let disabled = AnthropicProvider::new(String::new());
    assert!(enabled.is_enabled());
    assert!(!disabled.is_enabled());
}
