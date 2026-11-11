# llm-mcp user guide

llm-mcp lets an MCP-compatible application use OpenAI or OpenRouter to generate
content, classify evidence, create embeddings, and transcribe audio. This guide
covers installation, a first successful call, everyday tool use, and deployment.

It describes the **0.1.0** implementation and **MCP protocol 2025-11-25**. Model
names in examples are starting points: your provider account must have access to a
model that supports the requested operation, media, and options.

## Contents

1. [How the server works](#1-how-the-server-works)
2. [Install llm-mcp](#2-install-llm-mcp)
3. [Choose credentials and start](#3-choose-credentials-and-start)
4. [Connect a client and make your first call](#4-connect-a-client-and-make-your-first-call)
5. [Generate text, structured output, and function calls](#5-generate-text-structured-output-and-function-calls)
6. [Classify shared evidence](#6-classify-shared-evidence)
7. [Create text and contextual image embeddings](#7-create-text-and-contextual-image-embeddings)
8. [Transcribe audio](#8-transcribe-audio)
9. [Read results and handle errors](#9-read-results-and-handle-errors)
10. [Configure and tune the server](#10-configure-and-tune-the-server)
11. [Manage ChatGPT OAuth](#11-manage-chatgpt-oauth)
12. [Deploy with Docker, Compose, or Kubernetes](#12-deploy-with-docker-compose-or-kubernetes)
13. [Integrate with HTLK](#13-integrate-with-htlk)
14. [Operate, troubleshoot, test, and upgrade](#14-operate-troubleshoot-test-and-upgrade)
15. [CLI reference](#15-cli-reference)
16. [Glossary and further reading](#16-glossary-and-further-reading)

## 1. How the server works

The normal path is:

```text
Your application / MCP client
    -> llm-mcp tool call
        -> selected provider API
    <- completed structured result
```

An **MCP client** is the application that starts or connects to llm-mcp, discovers
its tools, and submits calls. A **provider** is OpenAI or OpenRouter. A **model**
is the provider-specific identifier supplied in each call.

llm-mcp is a server, with a management CLI. To generate an answer, call the MCP
tool `generate`; there is no `llm-mcp generate` CLI command.

### Available tools

| Tool | What it does | OpenAI API key | OpenRouter API key | OpenAI ChatGPT OAuth |
|---|---|---|---|---|
| `generate` | Responses generation, including text/images and returned function calls | Yes | Yes | Yes |
| `classify` | Multiple named questions over shared evidence | Yes | Yes, native alpha Decisions format | No |
| `embed` | Text/token embeddings, optionally prepared from image descriptions | Yes | Yes | No |
| `transcribe` | Base64 audio converted into a transcription upload | Yes | Yes, JSON formats only | No |

One instance uses **one provider and one authentication mode**, selected at startup.
Run separate instances if you need both providers or both OpenAI authentication
modes. An inactive provider's key is ignored; the server never falls back to it.

The caller selects models and native provider options. Most request fields and
full provider response objects pass through, including future fields, subject to
the server's execution, media, and authentication restrictions. Provider/model
compatibility is still checked upstream.

### State and responsibility

- Supply conversation history and manage application state in your client.
- Execute returned functions in your application, then send the results back.
- Apply classification thresholds and decide what an answer means for your workflow.
- Store/search embeddings in your own application or vector database.
- Keep provider credentials in the server environment or the protected OAuth store.

Each call returns one completed result. `stream` and `background` must be omitted
or `false`. HTTP may frame that completed result as an event stream; this does not
enable token-by-token output to callers. OAuth consumes a provider-required stream
internally and returns the completed object.

## 2. Install llm-mcp

Choose one installation method. Commands below use a POSIX shell on macOS/Linux
unless explicitly labeled PowerShell. Deployment commands assume you are in the
repository root. Using a released binary or container does not require Rust.

### Install with Cargo

Install Rust **1.98 or newer**, a native C/C++ build toolchain, and CMake for the
native cryptographic dependencies. Windows source builds need the MSVC build tools.
The repository follows stable Rust through `rust-toolchain.toml`.

If the crate version is available in your configured registry:

```sh
cargo install llm-mcp --locked
llm-mcp --version
llm-mcp --help
```

Cargo normally installs the executable in `$HOME/.cargo/bin` (Windows:
`%USERPROFILE%\.cargo\bin`). Ensure that directory is on `PATH`.

### Build or install from this checkout

```sh
cargo build --release --locked
./target/release/llm-mcp --version
# Install the checkout's executable on Cargo's executable path:
cargo install --path . --locked
```

On Windows, the built executable is `target\release\llm-mcp.exe`. If you do not
install it, substitute its full path for `llm-mcp` throughout this guide.

### Use a release binary

When release assets are published, obtain the archive and its SHA-256 checksum
from the repository's [GitHub Releases](https://github.com/thomasquintana/llm-mcp/releases).
Select the target for your operating system and CPU:

| System | Target |
|---|---|
| Linux x86-64 | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| Windows x86-64 | `x86_64-pc-windows-msvc` |

Linux releases require **glibc 2.36 or newer**. macOS archives are unsigned.
Verify the downloaded archive before extracting it. For example, replacing the
placeholder with the actual archive filename:

```sh
# Linux:
sha256sum -c <archive>.sha256
# macOS:
shasum -a 256 -c <archive>.sha256
```

In PowerShell, use `Get-FileHash -Algorithm SHA256 <archive>` and compare the hash
with the checksum file. Extract the archive, place the executable on `PATH`, then
run `llm-mcp --version`. Archives also include the README and license notices.

### Use a container

Build locally:

```sh
docker build -t llm-mcp:local .
docker run --rm llm-mcp:local --version
```

The examples also reference `ghcr.io/thomasquintana/llm-mcp:0.1.0` for installations
where that version has been published. Versioned OCI images support amd64 and
arm64. See [container deployment](#12-deploy-with-docker-compose-or-kubernetes).

## 3. Choose credentials and start

Choose one of the following paths. Settings are read once at startup; restart the
server after changing them. The executable does **not** load `.env` automatically.

### A. OpenAI API key: all four tools

1. Obtain an OpenAI Platform API key with access to your chosen models.
2. Supply it through your environment or secret manager. This illustrative shell
   assignment uses a placeholder, which you must replace:

   ```sh
   export OPENAI_API_KEY='replace-with-your-openai-key'
   export LLM_MCP_PROVIDER=openai
   export LLM_MCP_AUTH=api_key
   ```

3. Have your MCP client launch `llm-mcp serve` using stdio, or start an HTTP server:

   ```sh
   LLM_MCP_TRANSPORT=http LLM_MCP_BIND=127.0.0.1:8080 llm-mcp serve
   ```

4. Continue with [client connection](#4-connect-a-client-and-make-your-first-call).

The server checks that a key is configured at startup. Its validity and model
access are checked when a tool contacts the provider.

### B. OpenRouter API key: all four tools

```sh
export OPENROUTER_API_KEY='replace-with-your-openrouter-key'
export LLM_MCP_PROVIDER=openrouter
export LLM_MCP_AUTH=api_key
LLM_MCP_TRANSPORT=http llm-mcp serve
```

Use OpenRouter model identifiers, such as `openai/gpt-4.1-mini` for generation.
Classification uses a different format from OpenAI; see [classification](#6-classify-shared-evidence).
There is no fallback to `OPENAI_API_KEY` and no ChatGPT OAuth support on this path.

### C. ChatGPT OAuth: generation only

Stop any process using the same OAuth credential record, then:

```sh
export LLM_MCP_PROVIDER=openai
unset OPENAI_API_KEY
llm-mcp login
llm-mcp auth status
LLM_MCP_AUTH=oauth LLM_MCP_TRANSPORT=http llm-mcp serve
```

Login opens a browser and prints an authorization URL. Complete authorization on
the machine running login so its loopback callback is reachable. If the browser
does not open, use the printed URL. The callback waits up to five minutes.

Discovery now lists only `generate`. Use an account-available model, for example
the project's OAuth test model `gpt-6.1-sol`, with full history as an `input` array.
See [OAuth management](#11-manage-chatgpt-oauth) for parameter differences, remote
deployment, permissions, and refresh behavior.

### PowerShell environment equivalents

PowerShell uses assignments rather than POSIX inline environment syntax:

```powershell
$env:OPENAI_API_KEY = 'replace-with-your-openai-key'
$env:LLM_MCP_PROVIDER = 'openai'
$env:LLM_MCP_AUTH = 'api_key'
$env:LLM_MCP_TRANSPORT = 'http'
$env:LLM_MCP_BIND = '127.0.0.1:8080'
llm-mcp serve
```

For OAuth, remove the Platform key with
`Remove-Item Env:OPENAI_API_KEY -ErrorAction SilentlyContinue`, run login, and set
`$env:LLM_MCP_AUTH = 'oauth'`. Desktop clients may not inherit the environment of
your terminal; configure their server process environment explicitly.

## 4. Connect a client and make your first call

### Stdio: let the client launch the server

Configure these values in your client's MCP server settings:

| Setting | Value |
|---|---|
| Executable | Absolute path to `llm-mcp` if the client cannot resolve `PATH` |
| Arguments | `serve` |
| Transport | stdio |
| Environment | `LLM_MCP_TRANSPORT=stdio`, provider/auth selection, selected credentials |
| Protocol | `2025-11-25` |

For clients using a `mcpServers` configuration convention, this is an illustrative
OpenAI entry. The outer configuration format is client-specific:

```json
{
  "mcpServers": {
    "llm": {
      "command": "/absolute/path/to/llm-mcp",
      "args": ["serve"],
      "env": {
        "LLM_MCP_TRANSPORT": "stdio",
        "LLM_MCP_PROVIDER": "openai",
        "LLM_MCP_AUTH": "api_key",
        "OPENAI_API_KEY": "replace-with-your-openai-key"
      }
    }
  }
}
```

Use your client's secret injection mechanism where available. For OpenRouter,
change the provider and key variable. For OAuth, use `LLM_MCP_AUTH=oauth`, omit the
Platform key, and point to the previously authorized credential store if needed.

The client sends one JSON-RPC message per line to stdin. stdout is reserved for MCP
messages; logs go to stderr. A server started manually with default stdio waits
for a client handshake, so a quiet terminal is expected. Do not allocate a TTY for
container stdio connections.

### HTTP: connect to a running server

Use Streamable HTTP at **`http://127.0.0.1:8080/mcp`** for the local quick starts.
The endpoint is MCP JSON-RPC, not a REST `/generate` endpoint. Use a client that
supports the pinned protocol and session handling.

HTTP clients must preserve the `Mcp-Session-Id` returned at initialization and
send it on subsequent requests. Send `MCP-Protocol-Version: 2025-11-25` after
initialization and advertise both JSON and event-stream responses.

### Worked HTTP session with curl

The following POSIX-shell walkthrough works with the running HTTP server. It
requires curl. Its final `generate` call uses provider credentials and incurs
normal provider usage; initialization and discovery do not call the provider.

**1. Confirm process readiness.**

```sh
curl -i http://127.0.0.1:8080/health/ready
```

Expect HTTP 200. This reports server readiness, not credential validity or model
availability.

**2. Initialize and read the session header.**

```sh
curl -sS -i -N http://127.0.0.1:8080/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"user-guide","version":"1.0"}}}'
```

Expect a successful response identifying `llm-mcp` and protocol `2025-11-25`.
Copy the `mcp-session-id` response header into this shell variable:

```sh
SESSION='paste-the-returned-session-id'
```

The body may be JSON or SSE with `data:` lines containing JSON-RPC messages.

**3. Finish initialization.**

```sh
curl -sS -i http://127.0.0.1:8080/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-11-25' \
  -H "Mcp-Session-Id: $SESSION" \
  --data '{"jsonrpc":"2.0","method":"notifications/initialized"}'
```

Notifications have no JSON-RPC result; an accepted HTTP status is expected.

**4. Discover tools.**

```sh
curl -sS -N http://127.0.0.1:8080/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-11-25' \
  -H "Mcp-Session-Id: $SESSION" \
  --data '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
```

Expect four tools in API-key mode or only `generate` in OAuth mode.

**5. Generate an answer.** This request uses OpenAI API-key mode:

```sh
curl -sS -N http://127.0.0.1:8080/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-11-25' \
  -H "Mcp-Session-Id: $SESSION" \
  --data '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"generate","arguments":{"model":"gpt-4.1-mini","input":[{"role":"user","content":"Explain Rust ownership in one sentence."}],"store":false}}}'
```

For OpenRouter, change the model to `openai/gpt-4.1-mini`. For OAuth, select an
account-available model such as `gpt-6.1-sol`; the input-array form already fits.
Read `result.structuredContent` and check `result.isError` as explained in
[results](#9-read-results-and-handle-errors).

**6. Close the protocol session when finished.**

```sh
curl -sS -i -X DELETE http://127.0.0.1:8080/mcp \
  -H 'MCP-Protocol-Version: 2025-11-25' \
  -H "Mcp-Session-Id: $SESSION"
```

The rest of this guide shows **tool arguments only**, which go inside
`params.arguments` of a `tools/call` request. The MCP client normally constructs
that envelope for you.

## 5. Generate text, structured output, and function calls

### Basic text and reasoning

Call `generate` with:

```json
{
  "model": "gpt-4.1-mini",
  "input": [{"role": "user", "content": "Explain Rust ownership briefly."}],
  "store": false
}
```

For a model supporting reasoning controls, add a native option such as
`"reasoning":{"effort":"low"}`. There is no separate `reason` tool. Options such
as `max_output_tokens` may be supplied in OpenAI API-key mode if the model supports
them; they are not accepted by ChatGPT OAuth.

The full Responses object is returned. Read text from `output` items of type
`message`, then their `content` items of type `output_text`. Do not assume every
output item is text: function calls, reasoning, or other provider items may occur.
Usage and additional response fields remain available.

### JSON-Schema-constrained output

For a supporting model in API-key mode:

```json
{
  "model": "gpt-4.1-mini",
  "input": [{"role": "user", "content": "Summarize: Rust prevents many memory errors."}],
  "store": false,
  "text": {
    "format": {
      "type": "json_schema",
      "name": "summary",
      "strict": true,
      "schema": {
        "type": "object",
        "properties": {"summary": {"type": "string"}},
        "required": ["summary"],
        "additionalProperties": false
      }
    }
  }
}
```

The schema constrains the model's generated text. The outer tool result remains
the complete Responses object; extract the output text and parse it as JSON.
Provider refusal and model-specific support still need caller handling.

### Include an image

Replace the placeholder with the actual image's canonical base64:

```json
{
  "model": "gpt-4.1-mini",
  "input": [{
    "role": "user",
    "content": [
      {"type": "input_text", "text": "Describe the equipment and read its labels."},
      {"type": "input_image", "image_url": "data:image/png;base64,<encoded-image>"}
    ]
  }],
  "store": false
}
```

Supported image MIME types are `image/png`, `image/jpeg`, `image/webp`, and
`image/gif`. Use Responses content parts `input_text`/`input_image`.
Remote image URLs, file IDs, and local paths are rejected. The same base64 rule
applies to supported tool-output evidence, computer screenshots, and OpenAI
classification instructions. The provider checks the actual decoded image and
model compatibility.

To encode a local image on the **client**, this Python command emits a single-line
data URL (change both the filename and MIME type for other formats):

```sh
python3 -c 'import base64,pathlib; print("data:image/png;base64," + base64.b64encode(pathlib.Path("photo.png").read_bytes()).decode("ascii"))'
```

### Function calling and continuation

1. Include the function definition in a `generate` request.
2. Inspect returned `function_call` items and parse their JSON `arguments` strings.
3. Execute each function in your application.
4. Send the original history, returned output items, and matching
   `function_call_output` items in a follow-up request.

Example first call for an API-key model supporting function tools:

```json
{
  "model": "gpt-4.1-mini",
  "input": [{"role": "user", "content": "Look up the order status for order A123."}],
  "store": false,
  "tools": [{
    "type": "function",
    "name": "lookup_order",
    "description": "Look up an order by ID.",
    "parameters": {
      "type": "object",
      "properties": {"order_id": {"type": "string"}},
      "required": ["order_id"],
      "additionalProperties": false
    },
    "strict": true
  }],
  "tool_choice": {"type": "function", "name": "lookup_order"}
}
```

Illustrative follow-up arguments, assuming the provider returned this call item
and your application obtained the shown result:

```json
{
  "model": "gpt-4.1-mini",
  "store": false,
  "input": [
    {"role": "user", "content": "Look up the order status for order A123."},
    {"type": "function_call", "call_id": "call_example", "name": "lookup_order", "arguments": "{\"order_id\":\"A123\"}"},
    {"type": "function_call_output", "call_id": "call_example", "output": "{\"status\":\"shipped\"}"}
  ]
}
```

Use the real returned call item and `call_id`. In an actual continuation, retain
all relevant returned output items, including reasoning items when required by the
model, and resend any tool definitions/instructions needed for subsequent turns.
llm-mcp never executes the function itself.

OpenRouter requires full history: `store` must be omitted or `false`, and
`previous_response_id` omitted or `null`. OAuth also requires full history but
rejects the `previous_response_id` field even when null; its function/namespace
requirements are provider-specific. OpenAI API-key callers can use supported
provider-managed references where appropriate.

## 6. Classify shared evidence

`classify` evaluates multiple questions over shared input. The result is the full
native Decisions response, not a server-selected business action.

### OpenAI: input and a questions array

```json
{
  "model": "gpt-6-luna",
  "input": "Please refund the duplicate charge. I need this fixed immediately.",
  "questions": [
    {"name": "refund", "type": "predicate", "instructions": "Does the customer request a refund?"},
    {
      "name": "department",
      "type": "choice",
      "instructions": "Which department should handle this?",
      "choices": [
        {"value": "billing", "description": "Payments and refunds."},
        {"value": "support", "description": "Other requests."}
      ]
    },
    {
      "name": "urgency",
      "type": "score",
      "instructions": "How urgent is the request?",
      "levels": [
        {"label": "routine", "description": "No urgency expressed."},
        {"label": "urgent", "description": "Immediate attention requested."}
      ]
    }
  ]
}
```

Predicate questions return probabilities; choice and score questions retain their
native answer fields. Inspect the `answers` array and match names rather than
assuming a single answer. Per-question refusals are preserved alongside other
answers. Mixed text/image `input` uses the Responses-style content shown above.

### OpenRouter: state and a named questions object

OpenRouter uses its **alpha Decisions API**, whose format may evolve:

```json
{
  "model": "typesafe/jev-1.14",
  "state": "Please refund the duplicate charge. I need this fixed immediately.",
  "questions": {
    "refund": {"type": "noul", "instructions": "Does the customer request a refund?"},
    "department": {
      "type": "choice",
      "instructions": "Which department should handle this?",
      "criteria": {"billing": "Payments and refunds", "support": "Other requests"}
    },
    "urgency": {
      "type": "score",
      "instructions": "How urgent is the request?",
      "criteria": ["No urgency", "Needs attention soon", "Immediate attention"]
    }
  }
}
```

Supported types are `noul`, `choice`, `score`, and `box`. Each question needs
instructions. Choice uses named criteria; score uses an ordered criteria array;
box requires an appropriate model. Answers remain a native named object. For
example, inspect `answers.refund.noul` for a predicate-like probability and
`answers.department.choice`/`probabilities` for a native choice response.

OpenRouter classification images have a **nested** URL object, and text items in
state arrays are plain strings:

```json
{
  "model": "typesafe/jev-1.14",
  "state": [
    "Inspect the image.",
    {"type": "image_url", "image_url": {"url": "data:image/png;base64,<encoded-image>"}}
  ],
  "questions": {"red": {"type": "noul", "instructions": "Is there a red square?"}}
}
```

Alternatively, native PNG/JPEG assets use
`assets:{"reference-key":{"type":"image","format":"png","data_base64":"..."}}`;
reference each asset key in native context or question guidance. The API requires
assets to be used. State, guidance, and assets share the decoded-media budget.
Audio/video/file parts and remote image URLs are rejected. Image placement and
dimensions depend on the upstream model; the live test uses a 512×512 Jev fixture.
See the [OpenRouter reference](openrouter.md#classification-images) for details.

### Apply your decision policy

Choose thresholds using your application requirements and evaluation data. For
example, a workflow might route high-confidence refund requests to billing and
send uncertain/refused answers for review. A numerical probability is not an
instruction for the server to execute an action. llm-mcp preserves the evidence
assessment and leaves the action to the caller.

## 7. Create text and contextual image embeddings

`embed` requires API-key mode. Use your provider's embedding model identifier.

### Standard text or token input

```json
{
  "model": "text-embedding-3-small",
  "input": ["First document", "Second document"],
  "encoding_format": "float",
  "dimensions": 64
}
```

`input` accepts a string, a string batch, an unsigned token array, or a batch of
token arrays. `dimensions` and `encoding_format` are model-dependent. On OpenRouter,
an example model is `openai/text-embedding-3-small`.

The result is always an envelope:

```json
{
  "response": {
    "data": [{"index": 0, "embedding": [0.1, 0.2]}],
    "model": "example-model",
    "usage": {"total_tokens": 5}
  },
  "prepared_inputs": []
}
```

This is illustrative, shortened output; real vectors have the selected model's
dimensions. `response` is the complete provider object. Use provider result
indices to associate vectors with inputs. Standard `input` requests have no
preparation metadata, hence an empty `prepared_inputs`.

### Contextual image-description embeddings

Use `inputs` instead of `input`:

```json
{
  "model": "text-embedding-3-small",
  "inputs": [
    {"content": [
      {"type": "input_text", "text": "Equipment photograph for maintenance search."},
      {"type": "input_image", "image_url": "data:image/png;base64,<encoded-image>"}
    ]},
    {"content": [{"type": "input_text", "text": "Independent maintenance document."}]}
  ],
  "description": {
    "model": "gpt-4.1-mini",
    "instructions": "Retain visible equipment labels."
  }
}
```

Rules and processing:

1. `input` and `inputs` are mutually exclusive. `inputs` must be nonempty.
2. Each item has only a nonempty `content` array of `input_text`/`input_image` parts.
3. Any image-bearing item requires `description.model`. Its only other allowed
   configuration is optional string `description.instructions`.
4. The server describes all images in an item together, using its text as context.
   Visible objects, layout, labels, and distinguishing details are requested.
5. Text parts retain their order and are joined with two newlines. With context,
   the exact embedding input is:

   ```text
   Context:
   <original text context>

   Image description:
   <generated detailed description>
   ```

6. Image-only items embed the description alone. Text-only items embed their joined
   nonempty text directly.
7. All items are prepared before one final ordered embedding batch. Each item
   produces one vector. Use separate items when you need separate image vectors.

Prepared items include `index` and the exact submitted `text`. Image items also
include `description`, `description_model`, and `description_usage` when returned
by the provider. Final embedding usage is separate under `response.usage`.

These are **embeddings of descriptions**, not a native image-vector API. Preserve
prepared text if you need reproducibility; another run can produce a different
description. Use the same embedding model and compatible dimensions for documents
and search queries, and store vectors/index metadata in your application.

If preparation fails, the whole call fails and no final embedding batch is sent.
Earlier description requests may already have incurred usage. An `input_index`
identifies the failed item when available. Both stages use the selected provider,
one concurrency permit, and one shared deadline. OpenRouter final embedding routing
options do not automatically become image-description options.

## 8. Transcribe audio

`transcribe` requires API-key mode and a model supporting the requested output.

### Prepare and submit audio

Encode the file on your client; llm-mcp does not read a client-supplied server path:

```sh
python3 -c 'import base64,pathlib; print(base64.b64encode(pathlib.Path("meeting.mp3").read_bytes()).decode("ascii"))'
```

Put that output in `data_base64` with **no data URL prefix**:

```json
{
  "model": "gpt-4o-mini-transcribe",
  "file": {
    "filename": "meeting.mp3",
    "mime_type": "audio/mpeg",
    "data_base64": "<encoded-audio>"
  },
  "language": "en",
  "response_format": "json"
}
```

The server decodes the bytes and uploads multipart data to
`audio/transcriptions`, preserving filename and MIME type. The filename is a
basename, not a path. The file object accepts only the three shown fields.
URLs, file IDs, and malformed/empty base64 are rejected.

Supported filename extensions:
`flac`, `mp3`, `mp4`, `mpeg`, `mpga`, `m4a`, `ogg`, `wav`, `webm`.

Supported MIME types:
`audio/flac`, `audio/x-flac`, `audio/mpeg`, `audio/mp3`, `audio/mp4`, `audio/m4a`,
`audio/x-m4a`, `audio/ogg`, `audio/wav`, `audio/x-wav`, `audio/wave`, `audio/webm`,
`video/mp4`, `video/mpeg`, `video/webm`.

Keep metadata consistent with the real file. The provider validates file contents,
upload/model limits, language, and option combinations. There is no automatic
audio splitting.

### Output formats and options

| Provider | Formats |
|---|---|
| OpenAI | `json` (default), `verbose_json`, `diarized_json`, `text`, `srt`, `vtt` |
| OpenRouter | `json` (default), `verbose_json` |

Not every model supports every format. JSON formats return the full provider
object, preserving transcript, usage, timestamps, speakers, and unknown fields.
Text/subtitle formats return `{text, response_format}` with exact UTF-8 content,
including original whitespace. To obtain a subtitle file, save the returned
`text` string in your application.

Native options can include prompts, timestamp granularities, log probabilities,
and diarization when the selected model supports them. Scalar fields become text
multipart fields; scalar arrays become repeated `name[]` fields; nested objects
use bracketed names. Option keys must use letters, digits, or underscores.

For OpenAI diarization, a compatible request can select
`model:"gpt-4o-transcribe-diarize"`, `response_format:"diarized_json"`, and
`chunking_strategy:"auto"`. Known-speaker names and references use corresponding
arrays. Unlike the main file's raw base64, reference samples use audio data URLs,
such as `data:audio/wav;base64,<encoded-sample>`; their bytes count toward the media
limit. The provider checks sample-duration and model requirements.

OpenRouter can use a model such as `openai/whisper-large-v3`. Its `provider` and
`trace` options must be objects and become single JSON-encoded multipart fields.
Alternate `source_url` and `input_audio` sources are rejected. See
[media details](media.md#audio-transcription).

## 9. Read results and handle errors

### Successful tool results

MCP tool results contain:

- `isError:false` for success.
- `structuredContent`: the result object your application should consume.
- A text content block containing the same object serialized as JSON, for clients
  that consume text-only tool output.

The object inside `structuredContent` depends on the tool:

| Tool/output | Structured object |
|---|---|
| `generate` | Complete provider Responses object |
| `classify` | Complete native Decisions object |
| `embed` | `{response, prepared_inputs}` |
| `transcribe`, JSON formats | Complete provider JSON object |
| `transcribe`, text/subtitles | `{text, response_format}` |

Do not process both structured and text copies as independent results. The
duplicates increase wire size, so client transport limits need room for the full
MCP envelope, not just the provider response.

### Failed tool results

An illustrative tool failure, shown inside the JSON-RPC `result`:

```json
{
  "isError": true,
  "structuredContent": {
    "error": {
      "code": "rate_limited",
      "message": "provider returned HTTP 429",
      "retryable": true,
      "status": 429,
      "request_id": "example-request-id"
    }
  },
  "content": [{
    "type": "text",
    "text": "{\"error\":{\"code\":\"rate_limited\",\"message\":\"provider returned HTTP 429\",\"retryable\":true,\"status\":429,\"request_id\":\"example-request-id\"}}"
  }]
}
```

Every application error has `code`, `message`, and `retryable`. `status`,
`request_id`, and failed preparation `input_index` appear when available. Error
messages deliberately omit provider error bodies, request contents, and credentials.
Retain the correlation ID for provider-side investigation.

Malformed protocol messages and unknown tool names can produce JSON-RPC errors
rather than this tool-error envelope. HTTP framing/session failures may instead
be HTTP errors. Check all layers in a custom client.

| Error code | Meaning and next step |
|---|---|
| `configuration` | Fix environment values, missing selected key, or conflicting auth settings. |
| `invalid_request` | Correct the tool argument shape or unsupported execution flags. |
| `invalid_media` | Check MIME type, wrapper, filename, and canonical nonempty base64. |
| `tool_disabled` | Use an API-key instance for classify/embed/transcribe. |
| `unsupported_oauth_option` | Remove unsupported fields or use OpenAI API-key mode. |
| `authentication` | Check selected credentials/account access; OAuth may require reauthorization. |
| `credential_store` | Check regular files, permissions, writable storage, and imported host identity. |
| `credential_locked` | Stop the other process owning the OAuth record. |
| `rate_limited` | Provider returned 429; apply caller-managed backoff and replay policy. |
| `provider_invalid_request` | Provider returned 400/422; check model, options, media, and API contract. |
| `provider` | Other provider HTTP failure; inspect status and correlation ID. |
| `provider_protocol` | Invalid provider JSON/UTF-8 or malformed OAuth event stream. |
| `provider_incomplete` | Generation/stream did not reach a valid completed response. |
| `description_failed` | Image description was refused or returned no usable text; inspect input_index. |
| `request_too_large` | Encoded arguments, decoded media, prepared input, or upload exceeds a bound. |
| `response_too_large` | Provider response/event stream or assembled result exceeds its bound. |
| `timeout` | Queue-inclusive tool deadline or provider transport timeout. |
| `cancelled` | MCP cancellation stopped the local call. |
| `transport` | Provider connection or server transport failure. |
| `shutdown` | Server is stopping and cannot admit work or finish within shutdown bounds. |
| `internal` | Internal worker failure; retain logs and report reproducible context. |

### Retry and cancellation policy

The server does not automatically retry provider requests. HTTP 429 and 5xx errors
are marked retryable, as are provider transport failures and tool deadlines.
`retryable:true` means another attempt may succeed; it does **not** establish that
the first request did no work or incurred no charge. Choose retry counts, backoff,
and overall deadlines in your caller. OpenRouter's own native routing/fallback
preferences are separate from llm-mcp retries.

To cancel request ID `3`, an MCP client sends:

```json
{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3,"reason":"No longer needed"}}
```

Use the same HTTP session for that notification. Cancellation releases local
capacity and stops subsequent pipeline stages. It cannot undo provider work
already accepted. A running OAuth credential rotation finishes independently so
its replacement tokens can be saved safely.

## 10. Configure and tune the server

Environment variables are the only application configuration source: no config
file merging, profiles, or automatic dotenv loading. Model selection and reasoning
belong in tool arguments, not server configuration. Numeric limits must be positive
integers, in bytes or milliseconds as named below.

| Variable | Default | Purpose |
|---|---|---|
| `LLM_MCP_PROVIDER` | `openai` | `openai` or `openrouter`, one provider per instance |
| `LLM_MCP_AUTH` | `api_key` | `api_key` or OpenAI-only `oauth` |
| `LLM_MCP_TRANSPORT` | `stdio` | `stdio` or `http`; container default is `http` |
| `LLM_MCP_BIND` | `127.0.0.1:8080` | HTTP IP:port; container default is `0.0.0.0:8080` |
| `LLM_MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | Comma-separated accepted HTTP Host values; include actual service/ingress names |
| `OPENAI_API_KEY` | Unset | Required for OpenAI API-key serving; conflicts with OpenAI OAuth serving |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1/` | OpenAI inference base URL |
| `OPENAI_ORGANIZATION` | Unset | OpenAI Platform API-key organization header |
| `OPENAI_PROJECT` | Unset | OpenAI Platform API-key project header |
| `OPENROUTER_API_KEY` | Unset | Required for OpenRouter serving; no OpenAI key fallback |
| `OPENROUTER_BASE_URL` | `https://openrouter.ai/api/v1/` | OpenRouter inference base URL |
| `LLM_MCP_OAUTH_CREDENTIALS_PATH` | `$HOME/.config/llm-mcp/auth.json` | OAuth store; falls back to USERPROFILE then current directory for home; container uses `/state/auth.json` |
| `LLM_MCP_TIMEOUT_MS` | `120000` | Total tool deadline, including queueing and result assembly |
| `LLM_MCP_MAX_CONCURRENCY` | `16` | Simultaneous admitted tool executions per instance |
| `LLM_MCP_MAX_REQUEST_BYTES` | `8388608` (8 MiB) | Encoded arguments, aggregate decoded media, and each outbound provider request |
| `LLM_MCP_MAX_RESPONSE_BYTES` | `16777216` (16 MiB) | Each provider body/event stream and assembled result |
| `RUST_LOG` | `llm_mcp=info` | Tracing filter; stderr output |

Base URLs must be HTTP(S) without embedded credentials, a query, or a fragment.
A missing trailing slash is normalized. Only the selected provider's settings are
used. OpenRouter receives no OpenAI organization/project headers and does not open
ChatGPT credentials. OAuth authorization/token/issuer endpoints remain fixed to
OpenAI even if the inference base URL is changed.

Endpoint routing relative to the selected base:

| Operation | Endpoint |
|---|---|
| Generate and image descriptions | `responses` |
| OpenAI classify | `decisions` |
| OpenRouter classify | `/api/alpha/decisions` on the configured origin, replacing its base path |
| Embed | `embeddings` |
| Transcribe | `audio/transcriptions` |

### Size planning

- Base64 expands raw bytes by about 4/3. The default 8 MiB argument bound permits
  roughly 6 MiB of raw media, less JSON and metadata.
- Decoded media share an aggregate bound within a call. Outbound JSON and multipart
  overhead count too. Generated embedding text can exceed the outbound bound even
  if original arguments fit.
- MCP framing has an additional 64 KiB allowance. Oversized HTTP bodies receive
  413; oversized stdio lines are discarded, which can appear as a missing reply.
- OAuth's response bound counts all provider SSE events, not just final text.
- Prepared embedding metadata and text/subtitle wrappers count toward assembled
  results. Increase client limits to allow both structured and text copies.

For a larger recording, for example:

```sh
LLM_MCP_MAX_REQUEST_BYTES=33554432 LLM_MCP_TRANSPORT=http llm-mcp serve
```

This raises the server's bound to 32 MiB; upstream upload/model limits still apply.

### Deadlines and concurrency

A tool's deadline covers waiting for a permit, validation/decoding, provider
requests, all image-description steps, embedding/upload, and result construction.
An image pipeline holds one permit for its entire execution and works even with
concurrency set to one. It can make several sequential provider requests.

Increase timeout for long recordings or larger image batches; increase concurrency
only within provider quotas and instance capacity. Each replica has an independent
budget, so two API-key replicas with the default limit admit up to 32 executions.
Set client/proxy deadlines with enough allowance for the server to return its
completed result or structured timeout.

## 11. Manage ChatGPT OAuth

This section applies only to OpenAI. Stop the server before running management
commands against its credential record, including `auth status`, because those
commands also acquire exclusive ownership.

### Login, status, logout

```sh
llm-mcp login
llm-mcp auth status
# After stopping a running server, remove local authorization:
llm-mcp logout
```

Login uses browser authorization with PKCE, loopback callback, state/nonce checks,
and validated ID tokens. The issued registration belongs to one account/workspace.
Log out before authorizing a different account.

Status returns token-free JSON fields:

| Field | Meaning |
|---|---|
| `signed_in` | A local credential or pending recovery record exists |
| `host_id` | Stable installation identity |
| `expires_at` | Stored access-token expiry as Unix seconds, or null |
| `rotation_pending` | Replacement credentials still need admission/persistence |

Status does not contact OpenAI or refresh tokens. It opens the store and can create
its directory, lock, and installation identity, even if no account is signed in.
An expired access token does not itself mean the session is unusable: a valid
refresh token may renew it on the next generation request.

Logout clears local credentials and pending state but retains the installation
identity. To revoke authorization remotely, revoke the agent in ChatGPT settings.

### Storage and rotation

With the default `auth.json` path, adjacent state files are:

| File | Role |
|---|---|
| `auth.json` | Registration and rotating tokens |
| `auth.host.json` | Stable destination installation identity |
| `auth.lock` | Exclusive process ownership |
| `auth.pending.json` | Replacement credentials awaiting durable admission/recovery |

Unix credential records must be regular files with owner-only permissions, normally
0600. Windows uses a protected ACL restricted to the current user and SYSTEM.
The directory must be writable for atomic replacement and refresh. Mounting only a
read-only credential file is insufficient.

Refresh is demand-driven when a token is within roughly a minute of expiry.
Concurrent callers share one refresh operation. Rotated replacements are staged
before fallible validation/final persistence. On restart, pending recovery resumes
without re-exchanging the already-consumed refresh token. Preserve pending state
when diagnosing storage/network problems. Terminal authorization failures require
reauthorization; transient failures are shared with a short cooldown.

One process/replica must own each record. Do not delete a lock file to work around
a running owner; stop that process and let its ownership release normally.

### Import credentials on a remote host

1. Complete login on a machine with a browser.
2. Stop use of that credential session on the source machine.
3. Securely transfer the trusted credential record with its required permissions.
4. With the destination server stopped, import into writable persistent storage:

   ```sh
   LLM_MCP_OAUTH_CREDENTIALS_PATH=/state/auth.json llm-mcp auth import /transfer/auth.json
   ```

5. Start the destination with `LLM_MCP_AUTH=oauth` and no `OPENAI_API_KEY`.

Import preserves/creates destination identity rather than copying the source host
ID. Let the destination own all subsequent refreshes. Container and Kubernetes
procedures appear in the next section.

### OAuth request restrictions

- Only `generate` is advertised/enabled.
- `input` must be an array containing full context.
- `store` must be omitted or `false`; the server sets `false` internally.
- Omit `stream`/`background`, or set them to `false`; the server adapts the provider
  stream internally.
- The following fields are rejected even if supplied as null:
  `conversation`, `previous_response_id`, `max_output_tokens`, `max_tool_calls`,
  `metadata`, `moderation`, `multi_agent`, `prompt`, `prompt_cache_retention`,
  `safety_identifier`, `temperature`, `top_logprobs`, `top_p`, `truncation`, `user`.
- Function/custom namespaces and tool compatibility are validated by OpenAI.

An interrupted, failed, or incomplete stream is an error rather than a partial
answer. The adapter can reconstruct complete output from completed-item snapshots
when terminal metadata omits it. See [OAuth reference](oauth.md) for protocol details.

## 12. Deploy with Docker, Compose, or Kubernetes

The image runs as UID/GID **65532**, defaults to HTTP on `0.0.0.0:8080`, and uses
`/state/auth.json` for OAuth. Native binaries default to stdio. Set actual network
names in `LLM_MCP_ALLOWED_HOSTS`.

The server does not implement incoming MCP client authentication. Deploy HTTP on
your intended private network or behind your own authenticated access layer. The
Host allowlist validates request authorities; it is not user authentication.

### Standalone Docker

With `OPENAI_API_KEY` already exported and a local image built:

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  -e OPENAI_API_KEY llm-mcp:local
```

For OpenRouter:

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  -e LLM_MCP_PROVIDER=openrouter -e OPENROUTER_API_KEY llm-mcp:local
```

For a client-managed stdio connection, keep stdin open and omit a TTY:

```sh
docker run --rm -i -e OPENAI_API_KEY \
  -e LLM_MCP_TRANSPORT=stdio llm-mcp:local
```

When connecting by a container DNS name, add that name to the Host allowlist.
Use versioned images or digests in deployments.

### Compose with API keys

From a checkout, with the OpenAI key exported:

```sh
docker compose -f deploy/compose/api-key.yaml up -d
docker compose -f deploy/compose/api-key.yaml logs -f
```

The example binds only host loopback, uses a read-only root filesystem, drops
capabilities, and sets 135 seconds of stop grace. Replace its image with
`llm-mcp:local` if using a local build.

For OpenRouter, adapt the environment block:

```yaml
environment:
  OPENROUTER_API_KEY: ${OPENROUTER_API_KEY:?Set OPENROUTER_API_KEY}
  LLM_MCP_PROVIDER: openrouter
  LLM_MCP_AUTH: api_key
  LLM_MCP_ALLOWED_HOSTS: localhost,127.0.0.1,llm-mcp
```

Compose's own environment interpolation/`.env` support is separate from the
llm-mcp executable, which never loads dotenv files itself.

### Compose with OAuth

Complete local login and stop any owner of the source record. The import below
feeds the source record through stdin into a temporary owner-only file inside the
destination volume; replace the image if needed:

```sh
docker volume create llm-mcp-oauth
docker run --rm -i --read-only -v llm-mcp-oauth:/state \
  --entrypoint /bin/sh ghcr.io/thomasquintana/llm-mcp:0.1.0 -ec \
  'umask 077; trap "rm -f /state/import.json" EXIT; cat > /state/import.json; llm-mcp auth import /state/import.json' \
  < "$HOME/.config/llm-mcp/auth.json"
docker compose -f deploy/compose/oauth.yaml up -d
```

The Compose file uses that same explicitly named volume. Stop the service before
another import and use the actual source path if you configured a custom store.
Do not share this credential volume among replicas. The root filesystem can stay
read-only because `/state` is writable.

### Kubernetes with API keys

1. In your namespace, provision Secret `llm-mcp-openai` with key `api-key` using
   your secret-management process.
2. Review image version, namespace/service names in the Host allowlist, resources,
   and environment settings in the manifest.
3. Deploy:

   ```sh
   kubectl apply -f deploy/kubernetes/api-key.yaml
   kubectl rollout status deployment/llm-mcp
   kubectl get pods -l app=llm-mcp
   ```

The Service endpoint is `http://llm-mcp:8080/mcp` inside the namespace. The example
starts two replicas with ClientIP session affinity. Ingresses/load balancers must
preserve equivalent affinity because protocol sessions live in each process.

For OpenRouter, replace the key environment/Secret reference with
`OPENROUTER_API_KEY`, add `LLM_MCP_PROVIDER=openrouter`, and use matching catalogs
and model IDs.

### Kubernetes with OAuth: bootstrap in the right order

The OAuth manifest includes a PVC and a one-replica Deployment. Bootstrap by
creating the Deployment at **zero replicas** before applying it. This POSIX-shell
example requires Python 3 and PyYAML (`python3 -m pip install PyYAML` in your
chosen tooling environment):

```sh
python3 - <<'PY' | kubectl apply -f -
import pathlib
import yaml
documents = list(yaml.safe_load_all(pathlib.Path("deploy/kubernetes/oauth.yaml").read_text()))
for document in documents:
    if document.get("kind") == "Deployment":
        document["spec"]["replicas"] = 0
print(yaml.safe_dump_all(documents))
PY
```

For an existing deployment, scale it to zero and wait for its pods to terminate
before importing. Then:

1. Provision Secret `llm-mcp-oauth-transfer`, key `auth.json`, from the trusted
   local credential record after stopping its source owner. For example:

   ```sh
   kubectl create secret generic llm-mcp-oauth-transfer \
     --from-file=auth.json="$HOME/.config/llm-mcp/auth.json"
   ```

2. Run the one-time import and wait for success:

   ```sh
   kubectl apply -f deploy/kubernetes/oauth-import.yaml
   kubectl wait --for=condition=complete job/llm-mcp-oauth-import --timeout=300s
   kubectl logs job/llm-mcp-oauth-import
   ```

3. Delete the transfer Secret and start the server:

   ```sh
   kubectl delete secret llm-mcp-oauth-transfer
   kubectl scale deployment/llm-mcp-oauth --replicas=1
   kubectl rollout status deployment/llm-mcp-oauth
   ```

The PVC must support writes by UID/GID 65532. The example uses one replica,
`Recreate` updates, `fsGroup`, and an owner-run init container that restores file
mode 0600 after remount. The import Job writes destination-owned rotating state to
`/state`; it is not rerun on every restart. A storage class using delayed volume
binding may leave the PVC pending until the import Job is scheduled.

For reauthorization, stop/scale down the server first and create a fresh import
Job after any previous Job has been deleted or expired. Restore one replica only
after a successful import. The endpoint is `http://llm-mcp-oauth:8080/mcp`, and only
generation is available. Do not inject an OpenAI Platform key into this deployment.

### Probes, shutdown, and replicas

- `/health/live`: HTTP 200 while the process serves requests.
- `/health/ready`: HTTP 200 during normal operation; 503 when shutdown has been
  signaled while the probe can still be served.
- Neither endpoint contacts the provider or refreshes OAuth tokens.
- SIGTERM/Ctrl-C initiates shutdown. HTTP draining is bounded; configure stop grace
  **greater than the tool timeout plus ten seconds**. Example Kubernetes manifests
  use 140 seconds for the default 120-second tool timeout.
- API-key replicas scale independently but need HTTP session affinity. Reconnect
  and initialize after pod loss; do not blindly replay an uncertain call.
- OAuth uses one owner per record with writable persistent state. Do not scale a
  single credential record to multiple replicas.

See [deployment reference](deployment.md) for the checked-in deployment layout.

## 13. Integrate with HTLK

HTLK pins MCP protocol, implementation version, and tool descriptors. Export
catalogs using the **same provider, authentication mode, and transport** as the
server deployment:

```sh
llm-mcp catalog --htlk --transport stdio
llm-mcp catalog --htlk --transport streamable_http
LLM_MCP_PROVIDER=openrouter llm-mcp catalog --htlk --transport streamable_http
LLM_MCP_AUTH=oauth llm-mcp catalog --htlk --transport stdio
```

Exports print JSON to stdout and require no credentials. They still parse startup
configuration, so invalid provider/auth combinations or malformed settings fail.
`--transport streamable_http` describes an exported HTLK catalog; actual serving
uses the environment value `LLM_MCP_TRANSPORT=http`.

Checked-in catalog variants live under [`examples/htlk`](../examples/htlk/):

- `stdio.catalogs.json` / `streamable_http.catalogs.json`: OpenAI API-key mode.
- `*.openrouter.catalogs.json`: OpenRouter API-key mode.
- `*.oauth.catalogs.json`: OpenAI OAuth generation only.

Configure deployment `llm-mcp` in HTLK with the executable and `serve` for stdio,
or its Streamable HTTP `/mcp` URL. The catalog's tool server alias is `llm`.

[`workflow.htlk`](../examples/htlk/workflow.htlk) and
[`requests.json`](../examples/htlk/requests.json) demonstrate the four-tool OpenAI
workflow. Replace tiny illustrative media with real content. For OpenRouter,
adapt model IDs, classification fields, and transcription format; the default
request file is not an OpenRouter request set. OAuth has separate
[`oauth.htlk`](../examples/htlk/oauth.htlk) and
[`oauth.requests.json`](../examples/htlk/oauth.requests.json).

[`continuation.htlk`](../examples/htlk/continuation.htlk) shows caller-executed
function continuation. HTLK consumes `structuredContent` and applies its own
failure/retry policy. Allow for both wire-result copies and HTLK's tighter raw
response, graph output, numeric, and document limits.

After changes/upgrades, regenerate or check repository catalogs:

```sh
python3 tools/export_catalogs.py
python3 tools/export_catalogs.py --check
```

See [HTLK reference](htlk.md) for compiler/runtime verification commands.

## 14. Operate, troubleshoot, test, and upgrade

### Everyday checks

1. Confirm version and selected environment settings.
2. For HTTP, check readiness and MCP initialization separately.
3. Inspect tool discovery for the expected auth/provider contract.
4. Submit a small model-compatible request to check actual provider access.
5. Check `isError`, structured error fields, stderr/container logs, and request ID.

Logs default to `llm_mcp=info`. Use `RUST_LOG` to change the tracing filter and keep
stderr separate from MCP stdout in stdio clients. Health success alone never proves
that a provider call will succeed.

### Troubleshooting by symptom

| Symptom | Likely cause and remedy |
|---|---|
| Command not found | Add Cargo's bin directory to PATH or configure the client's absolute executable path. |
| Native server appears to hang quietly | Default stdio is waiting for MCP initialization; connect a client or explicitly select HTTP. |
| Client cannot see the key | Desktop/service environment differs from your shell; inject the selected key into the server process. |
| `.env` settings have no effect | Export them through your process launcher; llm-mcp does not load the file. |
| Startup requires a key despite local login | Auth defaults to api_key; explicitly select oauth for ChatGPT access. |
| OAuth startup reports a key conflict | Remove OPENAI_API_KEY from the OAuth process environment. |
| Only one tool is visible | Expected in OAuth mode; use an API-key instance for the other three. |
| HTTP connection refused/bind fails | Check transport, bind address, port ownership, and container port mapping. |
| HTTP Host rejected | Include the actual service/ingress hostname in LLM_MCP_ALLOWED_HOSTS. |
| HTTP protocol/session request fails | Use /mcp, both Accept types, matching protocol, initialized notification, and returned session ID; ensure replica affinity. |
| Session stops working after a restart | Initialize a new session; assess any uncertain in-flight call before retrying. |
| Protocol version mismatch | Use a client supporting 2025-11-25 and matching catalogs. |
| OpenRouter classify rejects arguments | Use state plus named questions with native types; OpenAI's input/questions array is a different contract. |
| Provider HTTP 400/422 | Check model ID/access, native options, image dimensions, and response format; use request ID for upstream diagnostics. |
| Image fails before upload | Use a supported MIME and canonical data URL; Responses and native OpenRouter Decisions use different image wrappers. |
| Audio fails before upload | Use raw base64 in file.data_base64, a basename with supported extension, and a supported MIME. |
| HTTP 413 or no reply for a large stdio line | Reduce encoded input or increase request/client bounds, accounting for framing and base64. |
| Image embed times out | One deadline includes every description and final batch; reduce items, adjust concurrency/timeout, and inspect input_index. |
| Credential record is locked | Stop its owning server/test/management command; a second process cannot share it. |
| Credential host mismatch | Transfer through auth import rather than copying a foreign record directly into place. |
| Credential permissions fail after remount | Restore owner-only file mode or Windows ACL; verify UID/storage and the provided permission-repair init container. |
| OAuth generation rejects options | Use an input array, omit unsupported fields completely, and use store:false. |
| OAuth readiness succeeds but generation fails | Health does not load/refresh usable authorization; check login/import, permissions, pending state, and account access. |
| Result exceeds client limit | Budget for structuredContent plus its text copy and the MCP envelope, and adjust server/graph limits together. |

### Verify a source installation

With dependencies installed, deterministic tests can be run without provider calls:

```sh
cargo test --all-features --locked -- --skip live_openai_ --skip live_openrouter_
```

Ordinary `cargo test --all-features --locked` also automatically runs real provider
suites whenever their credentials exist. Missing credentials produce visible
skips; existing invalid credentials or provider errors fail. Live tests incur
normal provider usage and OAuth tests may refresh the original credential record.
Stop other owners of that record first.

Choose one live suite explicitly:

```sh
cargo test --locked --test provider_e2e live_openai_api_key
cargo test --locked --test provider_e2e live_openrouter_api_key
cargo test --locked --test provider_e2e live_openai_oauth
```

Each suite exercises real MCP child processes over stdio and HTTP. The OAuth
suite covers generation and disabled tools; API-key suites cover their supported
operations. Model/deadline overrides, fixture provenance, gates, and failure
reporting are in the [testing reference](testing.md). List suites without accessing
credentials or making provider calls with:

```sh
cargo test --locked --test provider_e2e -- --list
```

Historical execution evidence is in [verification](verification.md); distinguish
mock-backed tests from live provider checks and actual registry/cluster deployment.

### Upgrade procedure

1. Read the [changelog](../CHANGELOG.md) and provider/authentication differences.
2. Record current executable/image version, application settings, and client limits.
3. Stop the instance cleanly; preserve OAuth writable state and installation
   identity if used. Avoid concurrent owners during migration.
4. Install the chosen binary/crate or update the pinned image version/digest.
5. Regenerate HTLK catalogs from the new executable and update pinned identity and
   descriptors with the matching transport/provider/authentication mode.
6. Restart, initialize a fresh session, check discovery, and make a small test call.
7. Confirm resource limits, probes, and shutdown grace still match your deployment.

Image examples use version tags, with no required moving `latest` tag. For release
artifact provenance, platform details, and publication recovery, see
[releases](releases.md). Developer checks are in [CONTRIBUTING](../CONTRIBUTING.md).

## 15. CLI reference

| Command | Purpose |
|---|---|
| `llm-mcp` | Same as serve when no subcommand is supplied |
| `llm-mcp serve` | Serve using startup environment settings |
| `llm-mcp login` | Complete browser-based OpenAI ChatGPT authorization |
| `llm-mcp logout` | Remove local OAuth credentials/pending state, retain host identity |
| `llm-mcp auth status` | Print token-free local status; does not refresh or call OpenAI |
| `llm-mcp auth import <source>` | Import a trusted credential file into the configured destination store |
| `llm-mcp catalog` | Print exact provider/auth-specific MCP identity and descriptors |
| `llm-mcp catalog --htlk` | Print HTLK catalog inputs, defaulting to stdio metadata |
| `llm-mcp catalog --htlk --transport streamable_http` | Export HTTP HTLK catalog metadata |
| `llm-mcp --help` / `llm-mcp <command> --help` | Show command/option usage |
| `llm-mcp --version` | Show executable version |

Catalog transport choices are `stdio` and `streamable_http`. Catalogs do not open
OAuth state or require credentials. Login/logout/auth commands are OpenAI-only;
unset OpenRouter selection or set `LLM_MCP_PROVIDER=openai` for them. Normal command
execution parses application configuration, so invalid unrelated settings can
also prevent a management command from starting. Command failures print diagnostics
to stderr and return a nonzero exit status.

## 16. Glossary and further reading

| Term | Meaning here |
|---|---|
| MCP | Model Context Protocol; the client/server protocol exposing tools |
| Tool arguments | The JSON object supplied as params.arguments in tools/call |
| Provider | OpenAI or OpenRouter, selected for an entire server instance |
| Model | Per-call provider identifier, with operation-specific capabilities |
| API-key mode | Server uses the selected provider's key and exposes all four tools |
| OAuth mode | OpenAI ChatGPT authorization; generation-only server |
| Stdio | Client-managed process communicating over stdin/stdout |
| Streamable HTTP | MCP transport at /mcp, with protocol sessions and JSON/SSE framing |
| SSE | Server-sent events; transport framing, also used internally for OAuth completion |
| Structured content | Machine-readable JSON result in MCP structuredContent |
| Catalog | Exported server identity/tool descriptors, including HTLK deployment metadata |
| Prepared input | Exact text constructed before an image-description embedding batch |
| Session affinity | Routing a protocol session back to the same server replica |

Focused references:

- [Environment configuration](configuration.md)
- [OpenRouter contracts and examples](openrouter.md)
- [Image/audio formats and limits](media.md)
- [OpenAI authentication](oauth.md)
- [Deployment](deployment.md)
- [HTLK integration](htlk.md)
- [Specification](specification.md)
- [Testing](testing.md) and [verification evidence](verification.md)
- [Releases](releases.md) and [contributing](../CONTRIBUTING.md)

The project is licensed under [Apache-2.0](../LICENSE). Dependency license texts
are included in [THIRD_PARTY_LICENSES](../THIRD_PARTY_LICENSES).
