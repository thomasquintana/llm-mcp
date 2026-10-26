//! Bounded incremental SSE decoding for OAuth Responses.

use crate::error::{Error, Result};
use reqwest::Response;
use serde_json::Value;

#[derive(Default)]
struct Decoder {
    buffer: Vec<u8>,
    data: Vec<u8>,
}

impl Decoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Option<Value>> {
        self.buffer.extend_from_slice(bytes);
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                if self.data.is_empty() {
                    continue;
                }
                let data = std::mem::take(&mut self.data);
                if data == b"[DONE]\n" {
                    continue;
                }
                let event: Value = serde_json::from_slice(&data)
                    .map_err(|_| Error::new("provider_protocol", "invalid OAuth SSE event JSON"))?;
                match event.get("type").and_then(Value::as_str) {
                    Some("response.completed") => {
                        let response = event
                            .get("response")
                            .filter(|v| {
                                v.is_object()
                                    && v.get("status").and_then(Value::as_str) == Some("completed")
                            })
                            .ok_or_else(|| {
                                Error::new("provider_protocol", "invalid completed response event")
                            })?;
                        return Ok(Some(response.clone()));
                    }
                    Some("response.failed" | "response.incomplete" | "error") => {
                        return Err(Error::new(
                            "provider_incomplete",
                            "OAuth generation failed or was incomplete",
                        ));
                    }
                    _ => {}
                }
            } else if let Some(data) = line.strip_prefix(b"data:") {
                self.data
                    .extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
                self.data.push(b'\n');
            }
        }
        Ok(None)
    }
}

/// Collect an OAuth stream until its required completed terminal event.
pub async fn collect(mut response: Response, limit: usize) -> Result<Value> {
    if !response
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|s| s.starts_with("text/event-stream"))
    {
        return Err(Error::new(
            "provider_protocol",
            "OAuth response must be an SSE stream",
        ));
    }
    let mut total = 0usize;
    let mut decoder = Decoder::default();
    while let Some(bytes) = response.chunk().await.map_err(Error::transport)? {
        if bytes.len() > limit.saturating_sub(total) {
            return Err(Error::new(
                "response_too_large",
                "OAuth stream exceeds configured byte limit",
            ));
        }
        total += bytes.len();
        if let Some(value) = decoder.push(&bytes)? {
            return Ok(value);
        }
    }
    Err(Error::new(
        "provider_incomplete",
        "OAuth stream ended before response.completed",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_every_byte_split_crlf_comments_and_unicode() {
        let bytes = ": heartbeat\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"é\"}\r\n\r\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[\"é\"],\"future\":true}}\r\n\r\n".as_bytes();
        for split in 0..bytes.len() {
            let mut decoder = Decoder::default();
            let first = decoder.push(&bytes[..split]).unwrap();
            let value = first
                .or_else(|| decoder.push(&bytes[split..]).unwrap())
                .unwrap();
            assert_eq!(value["output"][0], "é");
            assert_eq!(value["future"], true);
        }
    }

    #[test]
    fn terminal_failures_are_not_successes() {
        for name in ["response.failed", "response.incomplete", "error"] {
            let data = format!("data: {{\"type\":\"{name}\"}}\n\n");
            assert!(Decoder::default().push(data.as_bytes()).is_err());
        }
        assert!(Decoder::default().push(b"data: malformed\n\n").is_err());
        assert!(
            Decoder::default()
                .push(b"data: {\"type\":\"response.completed\",\"response\":{}}")
                .unwrap()
                .is_none()
        );
    }
}
