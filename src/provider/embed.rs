//! Contextual image descriptions followed by one ordered embedding batch.

use super::{Operation, Provider, bounded_json, media::Budget};
use crate::error::{Error, Result};
use serde_json::{Map, Value, json};

const INSTRUCTIONS: &str = "Produce a detailed textual description for semantic retrieval. Consider all supplied images together and use the accompanying text as context. Include relevant objects, relationships, layout, visible text, labels, and distinguishing details. Describe visible evidence accurately, distinguish uncertainty, and do not invent unseen details. Return only the description.";

struct Item {
    content: Value,
    context: String,
    images: bool,
}

struct Plan {
    request: Map<String, Value>,
    items: Vec<Item>,
    description: Option<Value>,
}

impl Provider {
    pub(super) async fn embed(&self, arguments: Value) -> Result<Value> {
        let limit = self.config.max_request_bytes;
        let plan = self.cpu(move || plan(arguments, limit)).await??;
        let mut request = plan.request;
        let mut texts = Vec::new();
        let mut prepared = Vec::new();
        let max_request = self.config.max_request_bytes;
        let max_response = self.config.max_response_bytes;
        for (index, item) in plan.items.into_iter().enumerate() {
            let entry = if item.images {
                let config = plan
                    .description
                    .as_ref()
                    .expect("validated description model");
                let model = config["model"]
                    .as_str()
                    .expect("validated model")
                    .to_owned();
                let instructions = match config.get("instructions").and_then(Value::as_str) {
                    Some(extra) => {
                        format!("{INSTRUCTIONS}\n\nAdditional description guidance:\n{extra}")
                    }
                    None => INSTRUCTIONS.to_owned(),
                };
                let response = self
                    .json(
                        Operation::Generate,
                        json!({
                            "model":model, "instructions":instructions,
                            "input":[{"role":"user", "content":item.content}], "store":false,
                        }),
                    )
                    .await
                    .map_err(|error| error.at_input(index))?;
                self.cpu(move || {
                    let description = description_text(&response).map_err(|error| error.at_input(index))?;
                    let text = if item.context.is_empty() { description.clone() }
                        else { format!("Context:\n{}\n\nImage description:\n{}", item.context, description) };
                    let mut entry = json!({"index":index, "text":text, "description":description,
                        "description_model":response.get("model").unwrap_or(&Value::String(model))});
                    if let Some(usage) = response.get("usage") { entry["description_usage"] = usage.clone(); }
                    Ok(entry)
                }).await??
            } else {
                json!({"index":index, "text":item.context})
            };
            (texts, prepared) = self
                .cpu(move || {
                    texts.push(entry["text"].as_str().expect("prepared text").to_owned());
                    prepared.push(entry);
                    bounded_json(&texts, max_request, true)?;
                    bounded_json(&prepared, max_response, false)?;
                    Ok((texts, prepared))
                })
                .await??;
        }
        let request = self
            .cpu(move || {
                if !texts.is_empty() {
                    request.insert("input".into(), json!(texts));
                }
                Value::Object(request)
            })
            .await?;
        let response = self.json(Operation::Embed, request).await?;
        self.cpu(move || json!({"response":response, "prepared_inputs":prepared}))
            .await
    }
}

fn plan(arguments: Value, limit: usize) -> Result<Plan> {
    let mut request = arguments.as_object().expect("validated object").clone();
    request.remove("stream");
    request.remove("background");
    if !request
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Err(invalid(
            "model must be a nonempty embedding model identifier",
        ));
    }
    let description = request.remove("description");
    if let Some(config) = &description {
        let object = config
            .as_object()
            .ok_or_else(|| invalid("description must be an object"))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "model" | "instructions"))
            || !config
                .get("model")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty())
            || config.get("instructions").is_some_and(|v| !v.is_string())
        {
            return Err(invalid(
                "description requires a model and optional string instructions",
            ));
        }
    }
    let Some(inputs) = request.remove("inputs") else {
        if description.is_some() {
            return Err(invalid("description requires multimodal inputs"));
        }
        let input = request
            .get("input")
            .ok_or_else(|| invalid("input or inputs is required"))?;
        let valid = input.is_string()
            || input.as_array().is_some_and(|a| {
                !a.is_empty()
                    && (a.iter().all(Value::is_string)
                        || a.iter().all(Value::is_u64)
                        || a.iter().all(|v| {
                            v.as_array().is_some_and(|tokens| {
                                !tokens.is_empty() && tokens.iter().all(Value::is_u64)
                            })
                        }))
            });
        if !valid {
            return Err(invalid(
                "input must be text, text batches, tokens or token batches",
            ));
        }
        return Ok(Plan {
            request,
            items: vec![],
            description: None,
        });
    };
    if request.contains_key("input") {
        return Err(invalid("input and inputs are mutually exclusive"));
    }
    let inputs = inputs
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or_else(|| invalid("inputs must be a nonempty array"))?;
    let mut budget = Budget::new(limit);
    let mut items = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let item = (|| {
            let object = input
                .as_object()
                .ok_or_else(|| invalid("each item requires content"))?;
            if object.keys().any(|key| key != "content") {
                return Err(invalid("input items accept only content"));
            }
            let content = input
                .get("content")
                .and_then(Value::as_array)
                .filter(|parts| !parts.is_empty())
                .ok_or_else(|| invalid("content must be a nonempty array"))?;
            let mut text = Vec::new();
            let mut images = false;
            for part in content {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_image") => {
                        budget.image(part)?;
                        images = true;
                    }
                    Some("input_text") => text.push(
                        part.get("text")
                            .and_then(Value::as_str)
                            .ok_or_else(|| invalid("input_text requires text"))?,
                    ),
                    _ => return Err(invalid("content accepts only input_text and input_image")),
                }
            }
            if images && description.is_none() {
                return Err(invalid("image inputs require description.model"));
            }
            let context = text.join("\n\n");
            if !images && context.trim().is_empty() {
                return Err(invalid("text-only input must be nonempty"));
            }
            Ok(Item {
                content: Value::Array(content.clone()),
                context,
                images,
            })
        })()
        .map_err(|error: Error| error.at_input(index))?;
        items.push(item);
    }
    Ok(Plan {
        request,
        items,
        description,
    })
}

fn description_text(response: &Value) -> Result<String> {
    let mut text = Vec::new();
    for item in response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        for part in item
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match part.get("type").and_then(Value::as_str) {
                Some("refusal") => {
                    return Err(Error::new(
                        "description_failed",
                        "image description was refused",
                    ));
                }
                Some("output_text") => {
                    if let Some(value) = part.get("text").and_then(Value::as_str) {
                        text.push(value);
                    }
                }
                _ => {}
            }
        }
    }
    let text = text.join("\n");
    if text.trim().is_empty() {
        return Err(Error::new(
            "description_failed",
            "image description returned no usable text",
        ));
    }
    Ok(text)
}

fn invalid(message: &str) -> Error {
    Error::new("invalid_request", message)
}
