//! Bounded incremental SSE decoding for OAuth Responses.

use crate::error::{Error, Result};
use reqwest::Response;
use serde_json::Value;

#[derive(Default)]
struct Decoder {
    buffer: Vec<u8>,
    scan: usize,
    consumed: usize,
    data: Vec<u8>,
}

impl Decoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Option<Value>> {
        self.buffer.extend_from_slice(bytes);
        while let Some(relative) = self.buffer[self.scan..].iter().position(|b| *b == b'\n') {
            let end = self.scan + relative;
            let line = &self.buffer[self.consumed..end];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            self.scan = end + 1;
            self.consumed = end + 1;
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
        self.scan = self.buffer.len();
        if self.consumed > 0 {
            self.buffer.drain(..self.consumed);
            self.scan -= self.consumed;
            self.consumed = 0;
        }
        Ok(None)
    }
}

/// Collect an OAuth stream until its required completed terminal event.
pub async fn collect(
    mut response: Response,
    limit: usize,
    provider: &super::Provider,
) -> Result<Value> {
    // Live OpenAI OAuth responses can omit Content-Type. In that case validate
    // the bounded SSE body and its completed terminal event rather than reject
    // a valid response. An explicit incompatible media type remains an error.
    if let Some(content_type) = response.headers().get("content-type")
        && !content_type.to_str().is_ok_and(|value| {
            value.split(';').next().is_some_and(|media_type| {
                media_type.trim().eq_ignore_ascii_case("text/event-stream")
            })
        })
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
        let (next, result) = provider
            .cpu(move || {
                let result = decoder.push(&bytes);
                (decoder, result)
            })
            .await?;
        decoder = next;
        if let Some(value) = result? {
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

    #[test]
    fn large_single_line_event_in_tiny_chunks_is_decoded_without_rescanning_prefixes() {
        let padding = "x".repeat(1024 * 1024);
        let event = format!(
            "data: {{\"type\":\"response.completed\",\"response\":{{\"status\":\"completed\",\"padding\":\"{padding}\"}}}}\n\n"
        );
        let mut decoder = Decoder::default();
        let mut result = None;
        for chunk in event.as_bytes().chunks(17) {
            if let Some(value) = decoder.push(chunk).unwrap() {
                result = Some(value);
            }
        }
        assert_eq!(result.unwrap()["padding"], padding);
    }
}
