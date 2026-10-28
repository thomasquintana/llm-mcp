# Specification — v0.1

`generate` forwards provider-compatible OpenAI Responses requests. `classify`
forwards OpenAI Decisions requests with shared evidence and named predicate,
choice, and score questions. Requests and responses retain unknown provider
fields. Credentials, base URL and transport settings are environment configuration,
not tool arguments. Conversations are supplied by callers; the server stores no
conversation history and executes no returned function calls.

Generation and classification accept native text/image content parts. Images must
be nonempty base64 data URLs with supported image MIME types; remote URLs and image
file references are rejected before dispatch. The same rule covers supported tool
outputs and classification question instructions. Unrelated JSON is not scanned as
media. Encoded request and aggregate decoded-media bytes are bounded.

`embed` accepts standard OpenAI text/token `input` or a mutually exclusive `inputs`
batch of `{content:[input_text/input_image parts]}`. Image-bearing items require
`description.model` with optional `description.instructions`. All images within an
item are described together; original text context plus the detailed description
becomes one embedding input. Text-only items embed directly. The server prepares
every item before issuing one ordered `/embeddings` batch; any preparation failure
fails the entire call and skips embedding. Errors identify `input_index` when known.
The consistent result envelope is `{response: <full embedding response>,
prepared_inputs: [...]}`. Prepared items retain index and exact text; image items
also retain description, description_model and returned description_usage. Standard
text/token requests return an empty prepared_inputs array. Both stages use API keys
and share one deadline and concurrency permit.

MCP arguments and structured results are JSON objects. Both tools publish stable
input/output schemas. Successful structuredContent is the full provider response.
The HTLK integration requires MCP 2025-11-25 and matching catalog descriptors.

Reject `stream: true` and `background: true`. Each call returns a completed result.
OAuth generation internally uses OpenAI-required SSE and store:false, and observes
OAuth-specific parameter limitations. Explicit auth chooses ChatGPT OAuth or an
API key for the entire server. OAuth enables only generation; all other tools are
disabled. OAuth and a configured API key conflict. No automatic fallback.

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
