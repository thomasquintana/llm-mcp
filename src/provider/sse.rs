//! Bounded incremental SSE decoding for OAuth Responses.

use crate::error::{Error, Result};
use reqwest::Response;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Decoder {
    buffer: Vec<u8>,
    scan: usize,
    consumed: usize,
    data: Vec<u8>,
    expected_items: BTreeSet<usize>,
    completed_items: BTreeMap<usize, Value>,
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
                    Some("response.output_item.added") => {
                        self.expected_items.insert(output_index(&event)?);
                    }
                    Some("response.output_item.done") => {
                        let index = output_index(&event)?;
                        let item = event
                            .get("item")
                            .filter(|item| item.is_object())
                            .ok_or_else(|| {
                                Error::new("provider_protocol", "invalid completed output item")
                            })?;
                        if self
                            .completed_items
                            .get(&index)
                            .is_some_and(|previous| previous != item)
                        {
                            return Err(Error::new(
                                "provider_protocol",
                                "conflicting completed output items",
                            ));
                        }
                        self.expected_items.insert(index);
                        self.completed_items.insert(index, item.clone());
                    }
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
                        return self.finish(response.clone()).map(Some);
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

    fn finish(&mut self, mut response: Value) -> Result<Value> {
        // Some live OAuth terminal events carry metadata/usage but omit output.
        // Completed item events are full provider snapshots, including function
        // arguments and future fields; retain them rather than discard the answer.
        let output_missing = response
            .get("output")
            .is_none_or(|output| output.is_null() || output.as_array().is_some_and(Vec::is_empty));
        if output_missing && !self.expected_items.is_empty() {
            if self
                .expected_items
                .iter()
                .any(|index| !self.completed_items.contains_key(index))
            {
                return Err(Error::new(
                    "provider_incomplete",
                    "OAuth stream omitted a completed output item",
                ));
            }
            let mut output = Vec::with_capacity(self.completed_items.len());
            for (expected, (index, item)) in std::mem::take(&mut self.completed_items)
                .into_iter()
                .enumerate()
            {
                if index != expected {
                    return Err(Error::new(
                        "provider_incomplete",
                        "OAuth stream has incomplete output indices",
                    ));
                }
                output.push(item);
            }
            response
                .as_object_mut()
                .expect("validated response object")
                .insert("output".into(), Value::Array(output));
        }
        Ok(response)
    }
}

fn output_index(event: &Value) -> Result<usize> {
    event
        .get("output_index")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| Error::new("provider_protocol", "invalid output item index"))
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

    #[test]
    fn reconstructs_output_omitted_from_live_oauth_terminal_response() {
        let item = serde_json::json!({"id":"msg_live","type":"message","role":"assistant","phase":"final_answer","status":"completed","content":[{"type":"output_text","text":"llm-mcp OAuth OK.","annotations":[],"logprobs":[]}]});
        let mut expected = serde_json::json!({"id":"resp_live","status":"completed","output":[],"usage":{"output_tokens":11},"future_terminal_field":true});
        let wire = [
            serde_json::json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_live","type":"message","status":"in_progress","content":[]}}),
            serde_json::json!({"type":"response.output_item.done","output_index":0,"item":item}),
            serde_json::json!({"type":"response.completed","response":expected}),
        ].into_iter().map(|event| format!("data: {event}\n\n")).collect::<String>();
        let result = Decoder::default().push(wire.as_bytes()).unwrap().unwrap();
        expected["output"] = serde_json::json!([item]);
        assert_eq!(result, expected);
    }

    #[test]
    fn completed_function_items_are_preserved_in_output_index_order() {
        let call = serde_json::json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"lookup","arguments":"{\"query\":\"Rust\"}","future_item_field":true});
        let message = serde_json::json!({"type":"message","content":[{"type":"output_text","text":"Looking it up."}]});
        let wire = [
            serde_json::json!({"type":"response.output_item.done","output_index":1,"item":message}),
            serde_json::json!({"type":"response.output_item.done","output_index":0,"item":call}),
            serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[]}}),
        ].into_iter().map(|event|format!("data: {event}\n\n")).collect::<String>();
        let result = Decoder::default().push(wire.as_bytes()).unwrap().unwrap();
        assert_eq!(result["output"], serde_json::json!([call, message]));
    }

    #[test]
    fn missing_or_conflicting_output_items_are_not_successes() {
        let cases = [
            vec![
                serde_json::json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message"}}),
            ],
            vec![
                serde_json::json!({"type":"response.output_item.done","output_index":1,"item":{"type":"message"}}),
            ],
            vec![
                serde_json::json!({"type":"response.output_item.done","output_index":0,"item":{"type":"message"}}),
                serde_json::json!({"type":"response.output_item.done","output_index":0,"item":{"type":"different"}}),
            ],
        ];
        for mut events in cases {
            events.push(serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[]}}));
            let wire = events
                .into_iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect::<String>();
            assert!(Decoder::default().push(wire.as_bytes()).is_err());
        }
    }
}
