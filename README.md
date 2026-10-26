# llm-mcp

A Rust MCP server exposing **OpenAI Responses as `generate`** and **OpenAI Decisions
as `classify`**. Designed for HTLK, standalone containers, and Kubernetes.

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

Tool arguments are provider request objects, with caller-selected models and
options. Full conversation context, function calls/outputs, images where supported,
and newly introduced provider fields are preserved. Example `generate` arguments:

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

Successful results contain the **complete OpenAI response object in MCP
`structuredContent`**, plus the same JSON in a compatibility text block. Errors
use `isError:true` with an `error` object containing `code`, `message`, `retryable`
and optional provider `status`/`request_id`. Automatic provider retries are disabled.

Calls are stateless and return one completed result. `stream:true` and
`background:true` are explicitly rejected. Requests are bounded by configurable
deadlines, concurrency and size limits, and support MCP cancellation.

## Sign in with ChatGPT

```sh
llm-mcp login
llm-mcp auth status
LLM_MCP_GENERATE_AUTH=oauth llm-mcp serve
```

OAuth is an explicit alternative for generation. Classification uses a Platform API
key. OAuth generation consumes OpenAI's required stream internally and returns the
completed response. OAuth has different supported parameters from Platform API-key
access; see [login, refresh and remote deployment](docs/oauth.md).

## Deployment and integration

- [Standalone Docker/Compose and Kubernetes](docs/deployment.md)
- [HTLK catalogs and function continuation](docs/htlk.md)
- [Specification](docs/specification.md)
- [Development checks](CONTRIBUTING.md)
- [Publishing and binary/container releases](docs/releases.md)

MCP protocol compatibility is pinned to **2025-11-25** for HTLK. Export descriptors
using `llm-mcp catalog`, or HTLK inputs using `llm-mcp catalog --htlk`.

## License

Apache-2.0; see [LICENSE](LICENSE). Dependency license texts are included in
[THIRD_PARTY_LICENSES](THIRD_PARTY_LICENSES).
