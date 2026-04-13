use serde_json::{json, Value};

/// Translate an Anthropic `/v1/messages` body into an OpenAI `/v1/chat/completions` body.
pub fn translate_request(anthropic_body: &Value) -> Result<Value, String> {
    let model = anthropic_body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or("missing model")?
        .to_string();

    let mut messages: Vec<Value> = Vec::new();

    if let Some(system) = anthropic_body.get("system") {
        let system_text = match system {
            Value::String(s) => s.clone(),
            Value::Array(parts) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        if !system_text.is_empty() {
            messages.push(json!({ "role": "system", "content": system_text }));
        }
    }

    let anth_messages = anthropic_body
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for m in anth_messages {
        let role = m
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("user")
            .to_string();
        let content = match m.get("content") {
            Some(Value::String(s)) => Value::String(s.clone()),
            Some(Value::Array(parts)) => {
                let text = parts
                    .iter()
                    .filter_map(|p| {
                        if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                            p.get("text").and_then(|t| t.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                Value::String(text)
            }
            _ => Value::String(String::new()),
        };
        messages.push(json!({ "role": role, "content": content }));
    }

    let mut out = json!({
        "model": model,
        "messages": messages,
    });

    // Reasoning models (o1/o3/o4) use max_completion_tokens.
    let is_reasoning = anthropic_body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|m| m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4"))
        .unwrap_or(false);

    if let Some(mt) = anthropic_body.get("max_tokens").and_then(|v| v.as_i64()) {
        let key = if is_reasoning {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        out[key] = json!(mt);
    }

    for field in ["temperature", "top_p", "stream", "tools", "tool_choice"] {
        if let Some(v) = anthropic_body.get(field) {
            out[field] = v.clone();
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_simple_request() {
        let anth = json!({
            "model": "gpt-4o-mini",
            "max_tokens": 64,
            "system": "You are helpful.",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let out = translate_request(&anth).unwrap();
        assert_eq!(out["model"], "gpt-4o-mini");
        assert_eq!(out["max_tokens"], 64);
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful.");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Hello");
    }

    #[test]
    fn translates_content_parts_to_string() {
        let anth = json!({
            "model": "gpt-4o",
            "max_tokens": 10,
            "messages": [{
                "role": "user",
                "content": [{"type": "text", "text": "hi"}, {"type": "text", "text": " there"}]
            }]
        });
        let out = translate_request(&anth).unwrap();
        assert_eq!(out["messages"][0]["content"], "hi there");
    }

    #[test]
    fn uses_max_completion_tokens_for_o1() {
        let anth = json!({
            "model": "o1-preview",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "x"}]
        });
        let out = translate_request(&anth).unwrap();
        assert_eq!(out["max_completion_tokens"], 100);
        assert!(out.get("max_tokens").is_none());
    }
}
