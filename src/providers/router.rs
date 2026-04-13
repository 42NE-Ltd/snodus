use serde_json::Value;

/// Default keywords that bias routing toward the most capable model.
const COMPLEX_KEYWORDS: &[&str] = &[
    "refactor",
    "architect",
    "architecture",
    "design",
    "debug",
    "redesign",
];

/// Keywords that bias toward the cheapest model.
const SIMPLE_KEYWORDS: &[&str] = &["translate", "format", "list", "summarize", "rename"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingMethod {
    Rules,
    #[allow(dead_code)]
    Llm,
}

impl RoutingMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rules => "rules",
            Self::Llm => "llm",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub target_model: String,
    pub method: RoutingMethod,
    pub reason: String,
    pub latency_ms: i32,
}

/// Rule-based router: picks a target model from the request body.
pub struct RuleRouter {
    pub haiku_model: String,
    pub sonnet_model: String,
    pub opus_model: String,
    pub short_threshold: usize,
    pub long_threshold: usize,
}

impl Default for RuleRouter {
    fn default() -> Self {
        Self {
            haiku_model: "claude-haiku-4-5".into(),
            sonnet_model: "claude-sonnet-4-5".into(),
            opus_model: "claude-opus-4-5".into(),
            short_threshold: 200,
            long_threshold: 2000,
        }
    }
}

impl RuleRouter {
    /// Estimate input tokens from concatenated message/system text (chars / 4).
    fn estimate_input_tokens(body: &Value) -> (usize, String) {
        let mut text = String::new();
        if let Some(system) = body.get("system") {
            match system {
                Value::String(s) => text.push_str(s),
                Value::Array(parts) => {
                    for p in parts {
                        if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                            text.push_str(t);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(msgs) = body.get("messages").and_then(|v| v.as_array()) {
            for m in msgs {
                match m.get("content") {
                    Some(Value::String(s)) => text.push_str(s),
                    Some(Value::Array(parts)) => {
                        for p in parts {
                            if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                                text.push_str(t);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let tokens = text.len() / 4;
        (tokens, text)
    }

    pub fn route(&self, body: &Value) -> RoutingDecision {
        let start = std::time::Instant::now();
        let (tokens, text) = Self::estimate_input_tokens(body);
        let lower = text.to_lowercase();

        // Keyword rules win over size rules (stronger intent signal).
        if COMPLEX_KEYWORDS.iter().any(|k| lower.contains(k)) {
            return RoutingDecision {
                target_model: self.opus_model.clone(),
                method: RoutingMethod::Rules,
                reason: "keyword:complex".into(),
                latency_ms: start.elapsed().as_millis() as i32,
            };
        }
        if SIMPLE_KEYWORDS.iter().any(|k| lower.contains(k)) {
            return RoutingDecision {
                target_model: self.haiku_model.clone(),
                method: RoutingMethod::Rules,
                reason: "keyword:simple".into(),
                latency_ms: start.elapsed().as_millis() as i32,
            };
        }

        let target = if tokens < self.short_threshold {
            &self.haiku_model
        } else if tokens < self.long_threshold {
            &self.sonnet_model
        } else {
            &self.opus_model
        };

        RoutingDecision {
            target_model: target.clone(),
            method: RoutingMethod::Rules,
            reason: format!("input_tokens~{tokens}"),
            latency_ms: start.elapsed().as_millis() as i32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn short_input_routes_to_haiku() {
        let body = json!({"messages": [{"role": "user", "content": "hi"}]});
        let d = RuleRouter::default().route(&body);
        assert!(d.target_model.contains("haiku"));
    }

    #[test]
    fn long_input_routes_to_sonnet_or_opus() {
        let long_text = "x".repeat(1000); // ~250 tokens
        let body = json!({"messages": [{"role": "user", "content": long_text}]});
        let d = RuleRouter::default().route(&body);
        assert!(d.target_model.contains("sonnet"));
    }

    #[test]
    fn very_long_input_routes_to_opus() {
        let long_text = "x".repeat(12000); // ~3000 tokens
        let body = json!({"messages": [{"role": "user", "content": long_text}]});
        let d = RuleRouter::default().route(&body);
        assert!(d.target_model.contains("opus"));
    }

    #[test]
    fn refactor_keyword_routes_to_opus() {
        let body =
            json!({"messages": [{"role": "user", "content": "please refactor this function"}]});
        let d = RuleRouter::default().route(&body);
        assert!(d.target_model.contains("opus"));
        assert_eq!(d.reason, "keyword:complex");
    }

    #[test]
    fn translate_keyword_routes_to_haiku() {
        let body = json!({"messages": [{"role": "user", "content": "translate this to French"}]});
        let d = RuleRouter::default().route(&body);
        assert!(d.target_model.contains("haiku"));
        assert_eq!(d.reason, "keyword:simple");
    }
}
