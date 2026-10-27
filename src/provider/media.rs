//! Base64-only media checks at provider content boundaries.

use crate::error::{Error, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

pub(super) const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

pub(super) fn invalid() -> Error {
    Error::new(
        "invalid_media",
        "media must contain nonempty base64 data with a supported MIME type",
    )
}

pub(super) struct Budget {
    remaining: usize,
}

impl Budget {
    pub(super) fn new(limit: usize) -> Self {
        Self { remaining: limit }
    }

    pub(super) fn decode(&mut self, data: &str) -> Result<Vec<u8>> {
        // A canonical padded encoding can overestimate the decoded length by at
        // most two bytes. Bound the allocation before decoding untrusted input.
        if data.len() / 4 * 3 > self.remaining.saturating_add(2) {
            return Err(too_large());
        }
        let bytes = STANDARD.decode(data).map_err(|_| invalid())?;
        if bytes.is_empty() {
            return Err(invalid());
        }
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(too_large)?;
        Ok(bytes)
    }

    pub(super) fn data_url(&mut self, value: &str, types: &[&str]) -> Result<()> {
        let (prefix, encoded) = value.split_once(",").ok_or_else(invalid)?;
        let mime = prefix
            .strip_prefix("data:")
            .and_then(|s| s.strip_suffix(";base64"))
            .ok_or_else(invalid)?;
        if !types.contains(&mime) {
            return Err(invalid());
        }
        self.decode(encoded)?;
        Ok(())
    }

    pub(super) fn image(&mut self, part: &Value) -> Result<()> {
        if ["file_id", "url", "path", "filename"]
            .iter()
            .any(|field| part.get(field).is_some())
        {
            return Err(invalid());
        }
        let url = part
            .get("image_url")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        self.data_url(url, IMAGE_TYPES)
    }

    fn parts(&mut self, parts: &Value) -> Result<()> {
        if let Some(parts) = parts.as_array() {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_image") => self.image(part)?,
                    // Chat Completions is not the request format of these tools.
                    Some("image_url") => return Err(invalid()),
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

pub(super) fn validate_images(arguments: &Value, limit: usize) -> Result<()> {
    let mut budget = Budget::new(limit);
    if let Some(items) = arguments.get("input").and_then(Value::as_array) {
        for item in items {
            if let Some(parts) = item.get("content") {
                budget.parts(parts)?;
            }
            if let Some(parts) = item.get("output") {
                budget.parts(parts)?;
            }
        }
    }
    if let Some(questions) = arguments.get("questions").and_then(Value::as_array) {
        for question in questions {
            if let Some(parts) = question.get("instructions") {
                budget.parts(parts)?;
            }
        }
    }
    Ok(())
}

fn too_large() -> Error {
    Error::new(
        "request_too_large",
        "decoded media exceed configured byte limit",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding_checks_padding_empty_data_and_aggregate_bounds() {
        let mut budget = Budget::new(2);
        budget
            .data_url("data:image/png;base64,eA==", IMAGE_TYPES)
            .unwrap();
        budget.decode("eA==").unwrap();
        assert_eq!(budget.decode("eA==").unwrap_err().code, "request_too_large");
        for value in ["", "eA", "eA===", "eB==", "@@@@", "a==="] {
            assert_eq!(
                Budget::new(100).decode(value).unwrap_err().code,
                "invalid_media"
            );
        }
        assert_eq!(
            Budget::new(1).decode("eHh4eHh4").unwrap_err().code,
            "request_too_large"
        );
    }
}
