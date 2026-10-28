//! Base64 audio uploads and complete JSON/subtitle transcription results.

use super::{
    Operation, Provider,
    media::{self, Budget},
    read_bounded,
};
use crate::error::{Error, Result};
use reqwest::{
    header,
    multipart::{Form, Part},
};
use serde_json::{Value, json};

const AUDIO_TYPES: &[&str] = &[
    "audio/flac",
    "audio/x-flac",
    "audio/mpeg",
    "audio/mp3",
    "audio/mp4",
    "audio/m4a",
    "audio/x-m4a",
    "audio/ogg",
    "audio/wav",
    "audio/x-wav",
    "audio/wave",
    "audio/webm",
    "video/mp4",
    "video/mpeg",
    "video/webm",
];
const EXTENSIONS: &[&str] = &[
    "flac", "mp3", "mp4", "mpeg", "mpga", "m4a", "ogg", "wav", "webm",
];

impl Provider {
    pub(super) async fn transcribe(&self, arguments: Value) -> Result<Value> {
        let limit = self.config.max_request_bytes;
        let (form, format) = self.cpu(move || prepare(arguments, limit)).await??;
        let request = self.request("audio/transcriptions", false).await?;
        let request = self
            .cpu(move || {
                request
                    .header(
                        header::ACCEPT,
                        "application/json, text/plain, text/vtt, application/x-subrip",
                    )
                    .multipart(form)
            })
            .await?;
        let response = self.send(request).await?;
        let status = response.status().as_u16();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        let result = if matches!(format.as_str(), "json" | "verbose_json" | "diarized_json") {
            self.decode_response(Operation::Transcribe, response, false)
                .await
        } else {
            match read_bounded(response, self.config.max_response_bytes).await {
                Ok(bytes) => {
                    self.cpu(move || {
                        let text = String::from_utf8(bytes).map_err(|_| {
                            Error::new("provider_protocol", "OpenAI returned invalid UTF-8")
                        })?;
                        Ok(json!({"text":text, "response_format":format}))
                    })
                    .await?
                }
                Err(error) => Err(error),
            }
        };
        result.map_err(|error| error.context(status, request_id))
    }
}

fn prepare(arguments: Value, limit: usize) -> Result<(Form, String)> {
    let mut arguments = arguments.as_object().expect("validated object").clone();
    if !arguments
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Err(invalid(
            "model must be a nonempty transcription model identifier",
        ));
    }
    arguments.remove("stream");
    arguments.remove("background");
    let file = arguments
        .remove("file")
        .ok_or_else(|| invalid("file is required"))?;
    let file = file.as_object().ok_or_else(media::invalid)?;
    if file
        .keys()
        .any(|key| !matches!(key.as_str(), "filename" | "mime_type" | "data_base64"))
    {
        return Err(media::invalid());
    }
    let filename = file
        .get("filename")
        .and_then(Value::as_str)
        .filter(|name| {
            !name.contains(['/', '\\', '\r', '\n', '\0'])
                && name.rsplit_once('.').is_some_and(|(stem, ext)| {
                    !stem.is_empty() && EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
                })
        })
        .ok_or_else(media::invalid)?;
    let mime = file
        .get("mime_type")
        .and_then(Value::as_str)
        .filter(|mime| AUDIO_TYPES.contains(mime))
        .ok_or_else(media::invalid)?;
    let encoded = file
        .get("data_base64")
        .and_then(Value::as_str)
        .ok_or_else(media::invalid)?;
    let mut budget = Budget::new(limit);
    let bytes = budget.decode(encoded)?;
    if let Some(references) = arguments.get("known_speaker_references") {
        for reference in references.as_array().ok_or_else(media::invalid)? {
            budget.data_url(reference.as_str().ok_or_else(media::invalid)?, AUDIO_TYPES)?;
        }
    }
    let format = arguments
        .entry("response_format")
        .or_insert_with(|| json!("json"))
        .as_str()
        .filter(|format| {
            matches!(
                *format,
                "json" | "verbose_json" | "diarized_json" | "text" | "srt" | "vtt"
            )
        })
        .ok_or_else(|| invalid("unsupported transcription response_format"))?
        .to_owned();
    let part = Part::bytes(bytes)
        .file_name(filename.to_owned())
        .mime_str(mime)
        .map_err(|_| media::invalid())?;
    let mut form = Form::new().part("file", part);
    let mut fields = Vec::new();
    let overhead = form.boundary().len()
        + b"--\r\nContent-Disposition: form-data; name=\"\"\r\n\r\n\r\n".len();
    let mut remaining = limit;
    for (key, value) in arguments {
        validate_key(&key)?;
        fields_for(key, value, &mut fields, &mut remaining, overhead)?;
    }
    for (key, value) in fields {
        form = form.text(key, value);
    }
    Ok((form, format))
}

fn fields_for(
    name: String,
    value: Value,
    fields: &mut Vec<(String, String)>,
    remaining: &mut usize,
    overhead: usize,
) -> Result<()> {
    match value {
        Value::Null => {}
        Value::String(text) => push_field(name, text, fields, remaining, overhead)?,
        Value::Bool(_) | Value::Number(_) => {
            push_field(name, value.to_string(), fields, remaining, overhead)?
        }
        Value::Array(values) => {
            for (index, value) in values.into_iter().enumerate() {
                let nested = if value.is_object() || value.is_array() {
                    format!("{name}[{index}]")
                } else {
                    format!("{name}[]")
                };
                fields_for(nested, value, fields, remaining, overhead)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                validate_key(&key)?;
                fields_for(format!("{name}[{key}]"), value, fields, remaining, overhead)?;
            }
        }
    }
    Ok(())
}

fn push_field(
    name: String,
    text: String,
    fields: &mut Vec<(String, String)>,
    remaining: &mut usize,
    overhead: usize,
) -> Result<()> {
    let size = overhead
        .saturating_add(name.len())
        .saturating_add(text.len());
    *remaining = remaining.checked_sub(size).ok_or_else(|| {
        Error::new(
            "request_too_large",
            "multipart fields exceed configured byte limit",
        )
    })?;
    fields.push((name, text));
    Ok(())
}

fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(invalid(
            "multipart option names must contain only letters, digits and underscores",
        ));
    }
    Ok(())
}
fn invalid(message: &str) -> Error {
    Error::new("invalid_request", message)
}
