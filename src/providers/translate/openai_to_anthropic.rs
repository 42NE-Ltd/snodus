use serde_json::{json, Value};
use uuid::Uuid;

/// Translate an OpenAI `/v1/chat/completions` response into an Anthropic `/v1/messages` shape.
pub fn translate_response(openai_body: &Value) -> Result<Value, String> {
    let model = openai_body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let choice = openai_body
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .ok_or("missing choices[0]")?;

    let content_text = choice
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    let stop_reason = match choice
        .get("finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("stop")
    {
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        "stop" => "end_turn",
        other => other,
    };

    let usage = openai_body.get("usage").cloned().unwrap_or(json!({}));
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    Ok(json!({
        "id": format!("msg_{}", Uuid::new_v4().simple()),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type": "text", "text": content_text}],
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_basic_response() {
        let openai = json!({
            "id": "chatcmpl-abc",
            "model": "gpt-4o-mini",
            "choices": [{
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 3}
        });
        let out = translate_response(&openai).unwrap();
        assert_eq!(out["model"], "gpt-4o-mini");
        assert_eq!(out["content"][0]["text"], "Hello!");
        assert_eq!(out["stop_reason"], "end_turn");
        assert_eq!(out["usage"]["input_tokens"], 10);
        assert_eq!(out["usage"]["output_tokens"], 3);
    }

    #[test]
    fn maps_length_to_max_tokens() {
        let openai = json!({
            "model": "gpt-4o",
            "choices": [{"message": {"content": "x"}, "finish_reason": "length"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });
        let out = translate_response(&openai).unwrap();
        assert_eq!(out["stop_reason"], "max_tokens");
    }
}
