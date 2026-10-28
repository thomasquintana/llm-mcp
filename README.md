# llm-mcp

A Rust MCP server exposing OpenAI through **`generate`**, **`classify`**, **`embed`**,
and **`transcribe`**. Designed for HTLK, standalone containers, and Kubernetes.

## Install and run

```sh
cargo install llm-mcp --locked
# Or from this checkout:
cargo build --release --locked
```

Supply `OPENAI_API_KEY` through your environment or secret manager, then run
`llm-mcp serve`. The default transport is stdio; stdout contains only MCP traffic.
For HTTP:

```sh
LLM_MCP_TRANSPORT=http LLM_MCP_BIND=127.0.0.1:8080 llm-mcp serve
```

The HTTP endpoint is `/mcp`, with `/health/live` and `/health/ready` probes. Use
`llm-mcp --help` for CLI commands. Configuration is environment-only; see the
[complete variable reference](docs/configuration.md).

## Tools

Models and provider options are caller-selected. Generation/classification preserve
full provider request/response objects; embedding adds an optional image-description
pipeline, and transcription converts base64 audio into a multipart upload.

| Tool | API-key mode | OAuth mode | Provider operation |
|---|---|---|---|
| `generate` | Enabled | Enabled | Responses, with text/base64 image input |
| `classify` | Enabled | Disabled | Decisions, with text/base64 image input |
| `embed` | Enabled | Disabled | Text/token embeddings, optionally from image descriptions |
| `transcribe` | Enabled | Disabled | Audio transcription from base64 bytes |

Example `generate` arguments:

```json
{
  "model": "gpt-6.1-sol",
  "input": [{"role": "user", "content": "Explain Rust ownership briefly."}],
  "reasoning": {"effort": "low"},
  "store": false
}
```

`generate` supports text, JSON-Schema-constrained output and returned function calls.
The caller executes functions and supplies their results in a follow-up call.
Reasoning effort is a generation option, so there is no separate `reason` tool.

`generate` and `classify` accept mixed content in `input`, such as:

```json
[
  {"role":"user","content":[
    {"type":"input_text","text":"Describe the visible equipment."},
    {"type":"input_image","image_url":"data:image/png;base64,<encoded-image>"}
  ]}
]
```

Images must be base64 data URLs (PNG, JPEG, WebP or GIF), including in tool-output
evidence and classification instructions. Remote image URLs and image file IDs are
rejected. OpenAI validates the decoded image format and model compatibility.

Example `classify` arguments:

```json
{
  "model": "gpt-6-luna",
  "input": "Please refund the duplicate charge.",
  "questions": [
    {"name": "refund", "type": "predicate", "instructions": "Does the customer request a refund?"}
  ]
}
```

Classification supports multiple named predicate, choice and score questions over
shared input. The caller applies probabilities/confidence to its own decision
logic. Per-question refusals are retained alongside other answers.

### Embeddings

Standard text/token requests use OpenAI's embedding fields:

```json
{"model":"text-embedding-3-small","input":["First document","Second document"]}
```

For images, use `inputs` instead of `input` and select a description model:

```json
{
  "model": "text-embedding-3-small",
  "inputs": [
    {"content": [
      {"type":"input_text","text":"Equipment photograph for maintenance search."},
      {"type":"input_image","image_url":"data:image/png;base64,<encoded-image>"}
    ]}
  ],
  "description": {
    "model": "gpt-4.1-mini",
    "instructions": "Retain visible equipment labels."
  }
}
```

Each item produces one vector. Multiple images in an item are described together;
the **original context plus detailed description** becomes the embedding input.
Image-only items receive a general detailed description; text-only items embed
directly. All descriptions must succeed before the single final embedding batch.
Any preparation failure fails the call and skips embedding, although earlier
description requests may already have incurred usage.

Results use `{response, prepared_inputs}`: `response` is the complete embedding
provider object, and prepared items include their index and exact submitted text.
Image items also include `description`, `description_model` and returned
`description_usage`. Standard `input` requests have an empty `prepared_inputs` list.
These are embeddings of descriptions; [details and limits](docs/media.md).

### Transcription

```json
{
  "model": "gpt-4o-transcribe",
  "file": {
    "filename": "meeting.mp3",
    "mime_type": "audio/mpeg",
    "data_base64": "<encoded-audio>"
  },
  "language": "en",
  "response_format": "json"
}
```

Audio is base64 bytes with filename/MIME metadata, without a data URL prefix.
URLs, paths and file IDs are rejected. The server decodes the bytes and uploads
them to OpenAI's transcription API. Provider options support prompting, timestamps,
log probabilities and diarization where the selected model permits them. Known
speaker references use base64 audio data URLs. JSON responses are preserved;
text/SRT/VTT responses become `{text, response_format}` with exact content.
See [media formats, examples and limits](docs/media.md).

### Results and execution

Successful results are JSON objects in MCP `structuredContent`, plus the same JSON
in a text block. Generation/classification and JSON transcription retain the full
provider object; embedding and text transcription use the envelopes above. Errors
use `isError:true` with an `error` object containing `code`, `message`, `retryable`
and optional `status`, `request_id` and failed preparation `input_index`.
Automatic provider retries are disabled.

Calls are stateless and return one completed result. `stream:true` and
`background:true` are explicitly rejected. Requests are bounded by configurable
deadlines, concurrency and size limits, and support MCP cancellation.

## Sign in with ChatGPT

```sh
llm-mcp login
llm-mcp auth status
LLM_MCP_AUTH=oauth llm-mcp serve
```

Authentication is server-wide and mutually exclusive. API-key mode enables all
tools; OAuth mode enables only generation and rejects `OPENAI_API_KEY`. OAuth
generation consumes OpenAI's required stream internally and returns the
completed response. OAuth has different supported parameters from Platform API-key
access; see [login, refresh and remote deployment](docs/oauth.md).

## Deployment and integration

- [Standalone Docker/Compose and Kubernetes](docs/deployment.md)
- [HTLK catalogs and function continuation](docs/htlk.md)
- [Specification](docs/specification.md)
- [Development checks](CONTRIBUTING.md)
- [Credential-gated live OpenAI E2E tests](docs/testing.md)
- [Publishing and binary/container releases](docs/releases.md)
- [Acceptance evidence and verification boundaries](docs/verification.md)

MCP protocol compatibility is pinned to **2025-11-25** for HTLK. Export descriptors
using `llm-mcp catalog`, or HTLK inputs using `llm-mcp catalog --htlk`. Exports reflect
`LLM_MCP_AUTH` and require no credentials.

## License

Apache-2.0; see [LICENSE](LICENSE). Dependency license texts are included in
[THIRD_PARTY_LICENSES](THIRD_PARTY_LICENSES).
