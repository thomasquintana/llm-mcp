//! Native OpenRouter restrictions without narrowing future provider options.

use super::{
    Operation,
    media::{self, Budget, IMAGE_TYPES},
};
use crate::error::{Error, Result};
use serde_json::Value;

pub(super) fn validate(operation: Operation, arguments: &Value, limit: usize) -> Result<()> {
    if operation == Operation::Generate
        && (arguments
            .get("store")
            .is_some_and(|v| v != &Value::Bool(false))
            || arguments
                .get("previous_response_id")
                .is_some_and(|v| !v.is_null()))
    {
        return Err(Error::new(
            "invalid_request",
            "OpenRouter Responses requires store:false or omitted and no previous_response_id; supply full input history",
        ));
    }
    if operation != Operation::Classify {
        return Ok(());
    }
    if !arguments
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|v| !v.trim().is_empty())
        || !arguments
            .get("state")
            .is_some_and(|v| v.is_string() || v.is_object() || v.is_array())
        || !arguments
            .get("questions")
            .and_then(Value::as_object)
            .is_some_and(|q| !q.is_empty())
    {
        return Err(Error::new(
            "invalid_request",
            "OpenRouter classify requires model, state and a nonempty named questions object",
        ));
    }
    let mut budget = Budget::new(limit);
    content(&arguments["state"], &mut budget)?;
    for question in arguments["questions"]
        .as_object()
        .expect("validated questions")
        .values()
    {
        if !matches!(
            question.get("type").and_then(Value::as_str),
            Some("noul" | "choice" | "score" | "box")
        ) || !question
            .get("instructions")
            .is_some_and(|v| v.is_string() || v.is_object() || v.is_array())
        {
            return Err(Error::new(
                "invalid_request",
                "OpenRouter questions require instructions and type noul, choice, score or box",
            ));
        }
        content(&question["instructions"], &mut budget)?;
        if let Some(criteria) = question.get("criteria") {
            content(criteria, &mut budget)?;
        }
    }
    if let Some(assets) = arguments.get("assets") {
        for asset in assets.as_object().ok_or_else(media::invalid)?.values() {
            let object = asset.as_object().ok_or_else(media::invalid)?;
            if object
                .keys()
                .any(|key| !matches!(key.as_str(), "type" | "format" | "data_base64"))
                || asset.get("type").is_some_and(|v| v != "image")
                || !matches!(
                    asset.get("format").and_then(Value::as_str),
                    Some("png" | "jpg" | "jpeg")
                )
            {
                return Err(media::invalid());
            }
            budget.decode(
                asset
                    .get("data_base64")
                    .and_then(Value::as_str)
                    .ok_or_else(media::invalid)?,
            )?;
        }
    }
    Ok(())
}

// Native state/instructions/criteria are arbitrary structured content. Inspect
// media parts there, but never reinterpret unrelated top-level routing metadata.
fn content(value: &Value, budget: &mut Budget) -> Result<()> {
    match value {
        Value::Array(items) => {
            for item in items {
                content(item, budget)?;
            }
        }
        Value::Object(object) => {
            match value.get("type").and_then(Value::as_str) {
                Some("image_url") => {
                    if object
                        .keys()
                        .any(|k| !matches!(k.as_str(), "type" | "image_url"))
                    {
                        return Err(media::invalid());
                    }
                    let image = &value["image_url"];
                    let image = image.as_object().ok_or_else(media::invalid)?;
                    if image
                        .keys()
                        .any(|k| !matches!(k.as_str(), "url" | "detail"))
                    {
                        return Err(media::invalid());
                    }
                    budget.data_url(
                        image
                            .get("url")
                            .and_then(Value::as_str)
                            .ok_or_else(media::invalid)?,
                        IMAGE_TYPES,
                    )?;
                    return Ok(());
                }
                Some(
                    "input_image" | "input_audio" | "audio_url" | "input_video" | "video_url"
                    | "input_file" | "file" | "image",
                ) => return Err(media::invalid()),
                _ => {}
            }
            for child in object.values() {
                content(child, budget)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inline_images_and_assets_share_one_decoded_budget() {
        let arguments = json!({"model":"m","state":[{"type":"image_url","image_url":{"url":"data:image/png;base64,eHh4"}}],"questions":{"q":{"type":"noul","instructions":"Q"}},"assets":{"ref":{"format":"png","data_base64":"eA=="}}});
        assert_eq!(
            validate(Operation::Classify, &arguments, 3)
                .unwrap_err()
                .code,
            "request_too_large"
        );
        validate(Operation::Classify, &arguments, 4).unwrap();
    }
}
