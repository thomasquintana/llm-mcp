//! Live-provider scenarios and semantic checks, also exercised without credentials.

use super::live::{Credential, Failure, Result, structured};
use serde_json::{Value, json};
use std::time::Duration;

pub const API_SUITE: &str = "live_openai_api_key";
pub const OAUTH_SUITE: &str = "live_openai_oauth";
const CONTEXT: &str = "Catalog item: shapes-fixture-42.";
const DIMENSIONS: usize = 64;

pub struct Selection {
    pub list: bool,
    filter: Option<String>,
    skips: Vec<String>,
    exact: bool,
    ignored_only: bool,
}

impl Selection {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut selection = Self {
            list: false,
            filter: None,
            skips: Vec::new(),
            exact: false,
            ignored_only: false,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--list" => selection.list = true,
                "--exact" => selection.exact = true,
                "--ignored" => selection.ignored_only = true,
                "--skip" => selection.skips.push(
                    args.next()
                        .ok_or_else(|| Failure::new("--skip requires a filter"))?,
                ),
                "--test-threads" | "--color" | "--format" => {
                    args.next()
                        .ok_or_else(|| Failure::new("test argument requires a value"))?;
                }
                "--nocapture" | "--show-output" | "--quiet" | "-q" | "--test"
                | "--include-ignored" => {}
                _ if arg.starts_with("--test-threads=")
                    || arg.starts_with("--color=")
                    || arg.starts_with("--format=") => {}
                _ if arg.starts_with('-') => {
                    return Err(Failure::new("unsupported live-test argument"));
                }
                _ if selection.filter.is_none() => selection.filter = Some(arg),
                _ => return Err(Failure::new("only one test filter is supported")),
            }
        }
        Ok(selection)
    }

    pub fn includes(&self, name: &str) -> bool {
        let matches = |filter: &str| {
            if self.exact {
                name == filter
            } else {
                name.contains(filter)
            }
        };
        !self.ignored_only
            && self.filter.as_deref().is_none_or(matches)
            && !self.skips.iter().any(|filter| matches(filter))
    }
}

pub fn timeout(get: impl Fn(&str) -> Option<String>) -> Result<Duration> {
    let ms = get("LLM_MCP_E2E_TIMEOUT_MS")
        .map_or(Ok(120_000), |value| value.parse::<u64>())
        .ok()
        .filter(|ms| *ms > 0)
        .ok_or_else(|| Failure::new("LLM_MCP_E2E_TIMEOUT_MS must be a positive integer"))?;
    Ok(Duration::from_millis(ms))
}

pub struct Case {
    pub name: &'static str,
    pub tool: &'static str,
    pub model: String,
    pub arguments: Value,
    check: fn(&Value) -> Result<()>,
    disabled: bool,
}

impl Case {
    pub fn evaluate(&self, result: &Value) -> Result<()> {
        if self.disabled {
            require(
                result["isError"] == true
                    && result["structuredContent"]["error"]["code"] == "tool_disabled",
                "OAuth tool was not locally disabled",
            )
        } else {
            (self.check)(structured(result)?)
        }
    }
}

pub fn cases(credential: &Credential, get: impl Fn(&str) -> Option<String>) -> Result<Vec<Case>> {
    let oauth = matches!(credential, Credential::OAuth(_));
    let generate = model(
        &get,
        if oauth { "OAUTH" } else { "GENERATE" },
        if oauth { "gpt-6.1-sol" } else { "gpt-4.1-mini" },
    )?;
    let mut cases = vec![
        Case {
            name: "generate_text",
            tool: "generate",
            model: generate.clone(),
            arguments: json!({"model":generate, "input":[{"role":"user","content":"Reply with only the word ORCHID."}], "store":false}),
            check: check_generation_text,
            disabled: false,
        },
        Case {
            name: "generate_image",
            tool: "generate",
            model: generate.clone(),
            arguments: json!({"model":generate, "input":[{"role":"user","content":[{"type":"input_text","text":"Describe the colors and shapes and read the visible labels. Be concise."}, image()]}], "store":false}),
            check: check_generation_image,
            disabled: false,
        },
    ];
    if oauth {
        for tool in ["classify", "embed", "transcribe"] {
            cases.push(Case {
                name: tool,
                tool,
                model: "disabled".into(),
                arguments: json!({}),
                check: |_| Ok(()),
                disabled: true,
            });
        }
        return Ok(cases);
    }
    let classify = model(&get, "CLASSIFY", "gpt-6-luna")?;
    let embed = model(&get, "EMBED", "text-embedding-3-small")?;
    let description = model(&get, "DESCRIPTION", "gpt-4.1-mini")?;
    let transcribe = model(&get, "TRANSCRIBE", "gpt-4o-mini-transcribe")?;
    for (name, input, expected, check) in [
        (
            "classify_text",
            json!("A red square is to the left of a blue circle."),
            "left",
            check_classification_text as fn(&Value) -> Result<()>,
        ),
        (
            "classify_image",
            json!([{"role":"user","content":[{"type":"input_text","text":"Evaluate the image."},image()]}]),
            "42",
            check_classification_image as fn(&Value) -> Result<()>,
        ),
    ] {
        let (instructions, choices) = if expected == "left" {
            (
                "Which side of the blue circle is the red square on?",
                json!([{"value":"left"},{"value":"right"}]),
            )
        } else {
            (
                "Which number is printed below the red square?",
                json!([{"value":"42"},{"value":"17"}]),
            )
        };
        cases.push(Case { name, tool: "classify", model: classify.clone(), arguments: json!({"model":classify,"input":input,"questions":[
            {"name":"red_square","type":"predicate","instructions":"Does the input describe or show a red square?"},
            {"name":"selection","type":"choice","instructions":instructions,"choices":choices}
        ]}), check, disabled:false });
    }
    cases.push(Case { name:"embed_text", tool:"embed", model:embed.clone(), arguments:json!({"model":embed,"input":["A red square next to a blue circle.","A green triangle."],"dimensions":DIMENSIONS,"encoding_format":"float"}), check:check_text_embeddings, disabled:false });
    cases.push(Case { name:"embed_images", tool:"embed", model:format!("{embed} + {description}"), arguments:json!({"model":embed,"inputs":[
        {"content":[{"type":"input_text","text":CONTEXT},image(),image()]},
        {"content":[{"type":"input_text","text":"An independent text-only item."}]}
    ],"description":{"model":description,"instructions":"Explicitly include colors, shapes, printed labels and the number."},"dimensions":DIMENSIONS,"encoding_format":"float"}), check:check_image_embeddings, disabled:false });
    cases.push(Case { name:"transcribe_audio", tool:"transcribe", model:transcribe.clone(), arguments:json!({"model":transcribe,"file":{"filename":"shapes.wav","mime_type":"audio/wav","data_base64":include_str!("../fixtures/shapes.wav.base64").trim()},"language":"en","response_format":"json"}), check:check_transcription, disabled:false });
    Ok(cases)
}

fn model(get: &impl Fn(&str) -> Option<String>, kind: &str, default: &str) -> Result<String> {
    let key = format!("LLM_MCP_E2E_{kind}_MODEL");
    let model = get(&key).unwrap_or_else(|| default.to_owned());
    if model.is_empty()
        || model.len() > 128
        || !model.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b':' | b'@')
        })
    {
        return Err(Failure(format!(
            "{key} must be a nonempty model identifier"
        )));
    }
    Ok(model)
}

fn image() -> Value {
    json!({"type":"input_image","image_url":format!("data:image/png;base64,{}",include_str!("../fixtures/shapes.png.base64").trim())})
}

pub fn check_discovery(listed: &Value, credential: &Credential) -> Result<()> {
    let names: Vec<_> = listed
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| Failure::new("tools/list lacks tools array"))?
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();
    let expected = if matches!(credential, Credential::OAuth(_)) {
        vec!["generate"]
    } else {
        vec!["generate", "classify", "embed", "transcribe"]
    };
    require(
        names == expected,
        "discovered tools do not match authentication mode",
    )
}

fn generated(value: &Value) -> Result<String> {
    let mut text = Vec::new();
    for item in value
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
            if part["type"] == "refusal" {
                return Err(Failure::new("generation was refused"));
            }
            if part["type"] == "output_text"
                && let Some(text_part) = part.get("text").and_then(Value::as_str)
            {
                text.push(text_part);
            }
        }
    }
    let text = text.join(" ");
    require(
        !text.trim().is_empty(),
        "generation returned no usable text",
    )?;
    Ok(text)
}

fn check_generation_text(value: &Value) -> Result<()> {
    words(&generated(value)?, &["orchid"])
}
fn check_generation_image(value: &Value) -> Result<()> {
    words(
        &generated(value)?,
        &["red", "square", "blue", "circle", "42"],
    )
}

fn classification(value: &Value, expected: &str) -> Result<()> {
    let answers = value
        .get("answers")
        .and_then(Value::as_array)
        .ok_or_else(|| Failure::new("classification lacks answers"))?;
    require(
        answers.len() == 2,
        "classification returned wrong answer count",
    )?;
    let predicate = answers
        .iter()
        .find(|a| a["name"] == "red_square")
        .ok_or_else(|| Failure::new("classification lacks named predicate"))?;
    require(
        predicate["type"] == "predicate"
            && predicate["probability"]
                .as_f64()
                .is_some_and(|p| p.is_finite() && (0.8..=1.0).contains(&p)),
        "classification did not recognize the red square",
    )?;
    let choice = answers
        .iter()
        .find(|a| a["name"] == "selection")
        .ok_or_else(|| Failure::new("classification lacks named choice"))?;
    require(
        choice["type"] == "choice"
            && choice["choice"] == expected
            && choice["confidence"]
                .as_f64()
                .is_some_and(|p| p.is_finite() && (0.5..=1.0).contains(&p)),
        "classification chose the wrong side/printed number or low confidence",
    )
}

fn check_classification_text(value: &Value) -> Result<()> {
    classification(value, "left")
}
fn check_classification_image(value: &Value) -> Result<()> {
    classification(value, "42")
}

fn vectors(value: &Value) -> Result<()> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| Failure::new("embedding response lacks vectors"))?;
    require(data.len() == 2, "embedding returned wrong vector count")?;
    let mut indices = Vec::new();
    for item in data {
        let index = item["index"]
            .as_u64()
            .ok_or_else(|| Failure::new("embedding lacks input index"))?;
        indices.push(index);
        let vector = item
            .get("embedding")
            .and_then(Value::as_array)
            .ok_or_else(|| Failure::new("embedding is not a float vector"))?;
        require(
            vector.len() == DIMENSIONS,
            "embedding returned wrong dimensions",
        )?;
        let mut norm = 0.0;
        for number in vector {
            let number = number
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| Failure::new("embedding contains a non-finite/non-numeric value"))?;
            norm += number * number;
        }
        require(
            norm.is_finite() && norm > 0.0,
            "embedding returned a zero/invalid vector",
        )?;
    }
    indices.sort_unstable();
    require(
        indices == [0, 1],
        "embedding indices do not map to input items",
    )
}

fn check_text_embeddings(value: &Value) -> Result<()> {
    vectors(&value["response"])?;
    require(
        value["prepared_inputs"] == json!([]),
        "standard embeddings unexpectedly prepared input",
    )
}

fn check_image_embeddings(value: &Value) -> Result<()> {
    vectors(&value["response"])?;
    let prepared = value["prepared_inputs"]
        .as_array()
        .ok_or_else(|| Failure::new("image embedding lacks preparation metadata"))?;
    require(
        prepared.len() == 2 && prepared[0]["index"] == 0 && prepared[1]["index"] == 1,
        "image preparation indices/count do not match items",
    )?;
    let first = &prepared[0];
    let description = first["description"]
        .as_str()
        .ok_or_else(|| Failure::new("image embedding lacks description"))?;
    words(description, &["red", "square", "blue", "circle", "42"])?;
    require(
        first["text"] == format!("Context:\n{CONTEXT}\n\nImage description:\n{description}"),
        "image embedding lost original context or description",
    )?;
    require(
        first["description_model"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
            && first["description_usage"].is_object(),
        "image description model/usage metadata missing",
    )?;
    require(
        prepared[1]["text"] == "An independent text-only item.",
        "text-only batch item was changed",
    )
}

fn check_transcription(value: &Value) -> Result<()> {
    let text = value["text"]
        .as_str()
        .ok_or_else(|| Failure::new("transcription lacks text"))?;
    words(text, &["red", "square", "blue", "circle"])
}

fn words(text: &str, expected: &[&str]) -> Result<()> {
    let lower = text.to_lowercase();
    let words: Vec<_> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    require(
        expected.iter().all(|word| words.contains(word)),
        "provider output did not identify the expected words/colors/shapes/number",
    )
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Failure::new(message))
    }
}
