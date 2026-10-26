# Specification — v0.1

`generate` forwards provider-compatible OpenAI Responses requests. `classify`
forwards OpenAI Decisions requests with shared evidence and named predicate,
choice, and score questions. Requests and responses retain unknown provider
fields. Credentials, base URL and transport settings are environment configuration,
not tool arguments. Conversations are supplied by callers; the server stores no
conversation history and executes no returned function calls.

MCP arguments and structured results are JSON objects. Both tools publish stable
input/output schemas. Successful structuredContent is the full provider response.
The HTLK integration requires MCP 2025-11-25 and matching catalog descriptors.

Reject `stream: true` and `background: true`. Each call returns a completed result.
OAuth generation internally uses OpenAI-required SSE and store:false, and observes
OAuth-specific parameter limitations. Explicit auth chooses ChatGPT OAuth or an
API key for generation; classification uses an API key. No automatic fallback.

Application settings come only from environment variables. OAuth registration,
host identity and rotating tokens persist separately in protected files. Login,
logout, auth status and remote import manage one account per instance.

Support stdio and Streamable HTTP, cancellation, deadlines, bounded concurrency
and payloads, graceful shutdown, health probes and structured errors. Automatic
provider retries are disabled. Caller owns classification thresholds and workflow
orchestration. HTTP client authentication will be designed separately.

Deploy the same multiarch container standalone or on Kubernetes. API-key replicas
scale independently. Each OAuth record has one owner and writable persistent
storage. Release crates.io, standalone binaries and OCI images from gated tags.
