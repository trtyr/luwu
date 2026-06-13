//! Server-Sent Events (SSE) stream parser.
//!
//! Both OpenAI and Anthropic use SSE for streaming responses.
//! This module provides a shared parser that turns a raw byte stream
//! into individual SSE events.

use futures::StreamExt;
use reqwest::Response;

/// A single SSE event.
#[derive(Debug, Clone)]
pub struct SseEvent {
    /// The `data:` field content. May span multiple lines.
    pub data: String,
    /// The `event:` field, if present.
    pub event_type: Option<String>,
}

/// Parse an HTTP response body into a stream of SSE events.
///
/// SSE format is dead simple:
/// ```text
/// event: message_start
/// data: {"type":"message_start","message":{...}}
///
/// data: {"id":"chatcmpl-abc","choices":[{"delta":{"content":"Hello"}}]}
///
/// data: [DONE]
///
/// ```
///
/// Events are separated by blank lines. Each event has optional `event:` and
/// `data:` lines. We only care about `data` and `event`.
pub fn parse_sse_stream(
    response: Response,
) -> impl futures::Stream<Item = std::result::Result<SseEvent, reqwest::Error>> {
    let stream = response.bytes_stream();

    futures::stream::unfold(
        (stream, String::new()),
        |(mut stream, mut buffer)| async move {
            loop {
                // Try to extract a complete event from the buffer.
                if let Some(event) = extract_next_event(&mut buffer) {
                    return Some((Ok(event), (stream, buffer)));
                }

                // Need more data.
                match stream.next().await {
                    Some(Ok(chunk)) => {
                        let text = String::from_utf8_lossy(&chunk);
                        buffer.push_str(&text);
                    }
                    Some(Err(e)) => {
                        return Some((Err(e), (stream, buffer)));
                    }
                    None => {
                        // Stream ended — try to parse whatever's left.
                        if buffer.trim().is_empty() {
                            return None;
                        }
                        if let Some(event) = extract_next_event(&mut buffer) {
                            return Some((Ok(event), (stream, buffer)));
                        }
                        return None;
                    }
                }
            }
        },
    )
}

/// Try to extract the next SSE event from the buffer.
/// Returns the event if found, removing it (and its trailing blank line) from the buffer.
fn extract_next_event(buffer: &mut String) -> Option<SseEvent> {
    // SSE events are separated by double newlines (\n\n).
    let event_end = buffer.find("\n\n")?;

    let raw_event = buffer[..event_end].to_string();
    buffer.drain(..event_end + 2);

    parse_single_event(&raw_event)
}

/// Parse a single raw SSE event block into an SseEvent.
fn parse_single_event(raw: &str) -> Option<SseEvent> {
    let mut data_parts: Vec<String> = Vec::new();
    let mut event_type: Option<String> = None;

    for line in raw.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            data_parts.push(data.to_string());
        } else if let Some(data) = line.strip_prefix("data:") {
            // Some servers send "data:" without a trailing space.
            data_parts.push(data.to_string());
        } else if let Some(et) = line.strip_prefix("event: ") {
            event_type = Some(et.to_string());
        } else if let Some(et) = line.strip_prefix("event:") {
            event_type = Some(et.to_string());
        }
        // Ignore comments (lines starting with ':') and unknown fields.
    }

    if data_parts.is_empty() {
        return None;
    }

    let data = data_parts.join("\n");

    // OpenAI sends "data: [DONE]" to signal the end.
    if data.trim() == "[DONE]" {
        return None;
    }

    Some(SseEvent {
        data,
        event_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_single_event() {
        let mut buf = "data: hello\n\n".to_string();
        let event = extract_next_event(&mut buf).unwrap();
        assert_eq!(event.data, "hello");
        assert!(buf.is_empty());
    }

    #[test]
    fn test_extract_event_with_type() {
        let mut buf = "event: message\ndata: {\"foo\":1}\n\n".to_string();
        let event = extract_next_event(&mut buf).unwrap();
        assert_eq!(event.event_type.as_deref(), Some("message"));
        assert_eq!(event.data, "{\"foo\":1}");
    }

    #[test]
    fn test_extract_multiple_events() {
        let mut buf = "data: first\n\ndata: second\n\n".to_string();
        let e1 = extract_next_event(&mut buf).unwrap();
        assert_eq!(e1.data, "first");
        let e2 = extract_next_event(&mut buf).unwrap();
        assert_eq!(e2.data, "second");
    }

    #[test]
    fn test_skip_done_signal() {
        let mut buf = "data: [DONE]\n\n".to_string();
        assert!(extract_next_event(&mut buf).is_none());
    }

    #[test]
    fn test_multiline_data() {
        let mut buf = "data: line1\ndata: line2\n\n".to_string();
        let event = extract_next_event(&mut buf).unwrap();
        assert_eq!(event.data, "line1\nline2");
    }

    #[test]
    fn test_incomplete_buffer() {
        let mut buf = "data: notyet".to_string();
        assert!(extract_next_event(&mut buf).is_none());
        // Still there
        assert_eq!(buf, "data: notyet");
    }

    #[test]
    fn test_ignore_comments() {
        let mut buf = ": this is a comment\ndata: payload\n\n".to_string();
        let event = extract_next_event(&mut buf).unwrap();
        assert_eq!(event.data, "payload");
    }
}
