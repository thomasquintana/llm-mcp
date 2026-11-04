# OpenRouter

Choose one provider per server instance. Supply `OPENROUTER_API_KEY` through the
environment or a secret manager, then run:

```sh
LLM_MCP_PROVIDER=openrouter llm-mcp serve
# HTTP:
LLM_MCP_PROVIDER=openrouter LLM_MCP_TRANSPORT=http llm-mcp serve
# Credential-free catalog export:
LLM_MCP_PROVIDER=openrouter llm-mcp catalog --htlk --transport stdio
```

The default base URL is `https://openrouter.ai/api/v1/`. Override it using
`OPENROUTER_BASE_URL`; missing trailing slashes are normalized. `OPENAI_API_KEY`,
`OPENAI_BASE_URL`, organization and project settings are ignored for OpenRouter.
`LLM_MCP_AUTH` must be `api_key`. ChatGPT login, logout and auth commands are
OpenAI-only. The server does not load `.env` automatically.

All calls retain the server's limits, cancellation, queue-inclusive deadlines,
base64-only media policy and disabled automatic retries. Requests and responses
preserve unknown provider fields. Model and option compatibility is provider-owned.
OpenRouter's own routing/fallback preferences may be supplied in native requests.

## Generate

```json
{
  "model": "openai/gpt-4.1-mini",
  "input": [{"role":"user","content":"Explain Rust ownership briefly."}],
  "store": false
}
```

Uses `/api/v1/responses`. Supply full conversation history, including returned
function-call items and corresponding `function_call_output` items for continuation.
The caller executes functions. `store:true` and non-null `previous_response_id`
fail locally; `stream:true` and `background:true` also fail. JSON schemas,
reasoning and other native generation options remain caller-selected.

Images use the existing Responses format:

```json
{"type":"input_image","image_url":"data:image/png;base64,<encoded-image>"}
```

## Classify: native alpha Decisions

OpenRouter's classification contract differs from OpenAI's. It uses
`https://openrouter.ai/api/alpha/decisions`, or that same absolute path on the
configured origin. Alpha request/response formats may change.

```json
{
  "model": "typesafe/jev-1.14",
  "state": "A customer reports being charged twice and requests a refund.",
  "questions": {
    "refund": {"type":"noul","instructions":"Does the customer request a refund?"},
    "team": {
      "type":"choice",
      "instructions":"Which team should handle the request?",
      "criteria":{"billing":"Payment and charge issues","support":"Other issues"}
    },
    "urgency": {
      "type":"score",
      "instructions":"How urgent is this request?",
      "criteria":["Can wait","Needs attention soon","Immediate attention"]
    }
  }
}
```

`questions` is a nonempty object keyed by caller-owned names. Supported question
types are `noul`, `choice`, `score` and `box`; each requires `instructions`.
Choice uses an object of criteria; score uses an ordered criteria array. Box
questions require a supporting model/endpoint (such as Jev 1.14). Native answers,
probabilities, confidence, usage and unknown fields are returned without conversion
to OpenAI's predicate/answer-array format. Callers apply their own thresholds.

### Classification images

Native image parts have a nested URL object, unlike Responses image parts:

```json
{
  "model":"typesafe/jev-1.14",
  "state":[
    "Inspect this image.",
    {"type":"image_url","image_url":{"url":"data:image/png;base64,<encoded-image>"}}
  ],
  "questions":{"red":{"type":"noul","instructions":"Is there a red square?"}}
}
```

Text items in native state arrays are plain strings. Alternatively, use
`assets:{"unique-reference":{"type":"image","format":"png","data_base64":"..."}}`
and reference the key from state or question guidance. Asset formats are PNG/JPEG
(`png`, `jpg`, `jpeg`); `type:"image"` is optional. Each asset must be referenced.
Inline image parts and assets share one aggregate decoded-media budget.

Provider rules determine image placement, dimensions and question support. Jev
supports images in nested state and question instructions/criteria; many other
models accept images only as top-level state items. A 512×512 synthetic PNG was
verified with Jev 1.14; the original 480×260 test fixture was rejected upstream.
Remote image URLs, multipart asset references and malformed base64 fail locally.
Audio, video and file parts are not supported by this classification integration.

## Embed

```json
{"model":"openai/text-embedding-3-small","input":["First document","Second document"]}
```

The existing `inputs`/`description` image-description pipeline also works. Choose
an OpenRouter vision model for `description.model`, such as `openai/gpt-4.1-mini`.
Both stages use OpenRouter and one tool deadline/concurrency permit. The result
remains `{response, prepared_inputs}`. Native embedding options (including
`provider` routing preferences) apply to the final embedding request; description
configuration accepts only model and optional instructions.

## Transcribe

```json
{
  "model":"openai/whisper-large-v3",
  "file":{"filename":"recording.wav","mime_type":"audio/wav","data_base64":"<encoded-audio>"},
  "language":"en",
  "response_format":"json"
}
```

Uses multipart `/api/v1/audio/transcriptions`. Supply the existing file object;
alternate `source_url` and `input_audio` sources are rejected. Supported formats
are `json` (default) and `verbose_json`. Full JSON results, timestamps and usage are
retained. Native `provider` and `trace` options must be objects and are encoded as
single JSON multipart fields. Scalar arrays use repeated `name[]` fields.

## Verification and references

With `OPENROUTER_API_KEY` exported:

```sh
cargo test --locked --test provider_e2e -- live_openrouter_api_key --exact --nocapture
```

This runs real MCP child processes over stdio and HTTP. See [testing](testing.md)
for model overrides, usage, skips and error reporting, and
[verification](verification.md) for actual execution evidence.

- [Responses](https://openrouter.ai/docs/api/reference/responses/overview)
- [Embeddings](https://openrouter.ai/docs/api/reference/embeddings)
- [Decisions](https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-request)
- [Multimodal Decisions](https://openrouter.ai/docs/guides/community/multimodal-decisions)
- [Transcription](https://openrouter.ai/docs/api/api-reference/stt/create-transcription)
