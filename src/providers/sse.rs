use axum::body::Body;
use bytes::Bytes;
use futures_util::stream::StreamExt;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct SseUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub model: Option<String>,
}

type OnEndCallback = Box<dyn FnOnce(SseUsage) + Send + 'static>;

/// Wraps a streaming body and invokes `on_end(usage)` once the stream terminates.
/// Token counts are extracted from `message_start` (input) and `message_delta`
/// (output). `on_end` always fires at end-of-stream, even if usage is partial.
pub fn intercept<F>(body: Body, on_end: F) -> Body
where
    F: FnOnce(SseUsage) + Send + 'static,
{
    let usage = Arc::new(Mutex::new(SseUsage::default()));
    let buffer = Arc::new(Mutex::new(Vec::<u8>::new()));
    let on_end: Arc<Mutex<Option<OnEndCallback>>> = Arc::new(Mutex::new(Some(Box::new(on_end))));

    let usage_clone = usage.clone();
    let buffer_clone = buffer.clone();

    let stream = body.into_data_stream().map(move |chunk| {
        if let Ok(ref bytes) = chunk {
            let mut buf = buffer_clone.lock().unwrap();
            buf.extend_from_slice(bytes);
            parse_events_from_buffer(&mut buf, &mut usage_clone.lock().unwrap());
        }
        chunk.map_err(|e| std::io::Error::other(e.to_string()))
    });

    let finalized = FinalizingStream {
        inner: Box::pin(stream),
        on_end,
        usage,
    };

    Body::from_stream(finalized)
}

type BoxedStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

struct FinalizingStream {
    inner: BoxedStream,
    on_end: Arc<Mutex<Option<OnEndCallback>>>,
    usage: Arc<Mutex<SseUsage>>,
}

impl futures_util::Stream for FinalizingStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let poll = self.inner.as_mut().poll_next(cx);
        if let std::task::Poll::Ready(None) = &poll {
            if let Some(cb) = self.on_end.lock().unwrap().take() {
                let usage = self.usage.lock().unwrap().clone();
                cb(usage);
            }
        }
        poll
    }
}

impl Drop for FinalizingStream {
    fn drop(&mut self) {
        if let Some(cb) = self.on_end.lock().unwrap().take() {
            let usage = self.usage.lock().unwrap().clone();
            cb(usage);
        }
    }
}

/// Parse any complete events from the buffer and update `usage` in place.
/// Leaves any partial trailing event in the buffer.
fn parse_events_from_buffer(buf: &mut Vec<u8>, usage: &mut SseUsage) {
    // SSE events are separated by double newline (\n\n). Find complete events.
    loop {
        let Some(idx) = find_event_boundary(buf) else {
            return;
        };
        let event_bytes: Vec<u8> = buf.drain(..idx).collect();
        // Consume the boundary \n\n
        while buf.first() == Some(&b'\n') || buf.first() == Some(&b'\r') {
            buf.remove(0);
            if buf.first() == Some(&b'\n') || buf.first() == Some(&b'\r') {
                buf.remove(0);
                break;
            }
        }
        let event_str = String::from_utf8_lossy(&event_bytes);
        process_event(&event_str, usage);
    }
}

fn find_event_boundary(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

fn process_event(event: &str, usage: &mut SseUsage) {
    // Extract data: lines; the JSON is on a `data: {...}` line.
    for line in event.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match event_type {
            "message_start" => {
                if let Some(msg) = value.get("message") {
                    if let Some(model) = msg.get("model").and_then(|v| v.as_str()) {
                        usage.model = Some(model.to_string());
                    }
                    if let Some(u) = msg.get("usage") {
                        if let Some(input) = u.get("input_tokens").and_then(|v| v.as_i64()) {
                            usage.input_tokens = input as i32;
                        }
                        if let Some(output) = u.get("output_tokens").and_then(|v| v.as_i64()) {
                            usage.output_tokens = output as i32;
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(u) = value.get("usage") {
                    if let Some(output) = u.get("output_tokens").and_then(|v| v.as_i64()) {
                        usage.output_tokens = output as i32;
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_input_from_message_start() {
        let event = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":15,\"output_tokens\":1}}}";
        let mut usage = SseUsage::default();
        process_event(event, &mut usage);
        assert_eq!(usage.input_tokens, 15);
        assert_eq!(usage.output_tokens, 1);
        assert_eq!(usage.model.as_deref(), Some("claude-sonnet-4-5"));
    }

    #[test]
    fn updates_output_from_message_delta() {
        let event = "event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":142}}";
        let mut usage = SseUsage {
            input_tokens: 10,
            ..SseUsage::default()
        };
        process_event(event, &mut usage);
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 142);
    }

    #[test]
    fn parses_buffered_stream() {
        let stream = b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-haiku-4-5\",\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":42}}\n\n";
        let mut buf = stream.to_vec();
        let mut usage = SseUsage::default();
        parse_events_from_buffer(&mut buf, &mut usage);
        assert_eq!(usage.input_tokens, 5);
        assert_eq!(usage.output_tokens, 42);
        assert_eq!(usage.model.as_deref(), Some("claude-haiku-4-5"));
    }

    #[test]
    fn partial_event_stays_in_buffer() {
        let mut buf = b"event: message_start\ndata: {\"type\":\"message_start\",\"mess".to_vec();
        let mut usage = SseUsage::default();
        parse_events_from_buffer(&mut buf, &mut usage);
        assert!(!buf.is_empty());
        assert_eq!(usage.input_tokens, 0);
    }
}
