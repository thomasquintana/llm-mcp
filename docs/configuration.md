# Environment configuration

There is one configuration source: environment variables read at startup. No TOML,
profiles, dotenv loading or configuration merging. Tool arguments are provider
request fields; model selection and reasoning effort belong to each call.

| Variable | Default | Meaning |
|---|---|---|
| `LLM_MCP_TRANSPORT` | `stdio` | `stdio` or `http` |
| `LLM_MCP_BIND` | `127.0.0.1:8080` | HTTP IP and port; container default is `0.0.0.0:8080` |
| `LLM_MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | Comma-separated HTTP Host allowlist; configure actual service names |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1/` | Provider connection base URL; missing trailing slash is normalized |
| `OPENAI_API_KEY` | unset | Platform key for classification and API-key generation |
| `OPENAI_ORGANIZATION` | unset | Organization header on API-key requests |
| `OPENAI_PROJECT` | unset | Project header on API-key requests |
| `LLM_MCP_GENERATE_AUTH` | `api_key` | Explicitly `api_key` or `oauth` |
| `LLM_MCP_OAUTH_CREDENTIALS_PATH` | `$HOME/.config/llm-mcp/auth.json` | Writable OAuth state; container default `/state/auth.json` |
| `LLM_MCP_TIMEOUT_MS` | `120000` | Total call deadline including waiting for concurrency capacity |
| `LLM_MCP_MAX_CONCURRENCY` | `16` | In-flight provider calls per instance |
| `LLM_MCP_MAX_REQUEST_BYTES` | `8388608` | Serialized provider request bound |
| `LLM_MCP_MAX_RESPONSE_BYTES` | `16777216` | Provider body/SSE stream byte bound |
| `RUST_LOG` | `llm_mcp=info` | Tracing filter; logs go to stderr |

Numeric limits must be positive integers. Base URLs must be HTTP(S), without
embedded credentials, query parameters or fragments. Organization/project headers
apply only to Platform API-key requests. OAuth token endpoints and issuer are fixed
to OpenAI's published authentication service.

MCP wire requests have an additional 64 KiB framing allowance. Oversized HTTP
requests receive HTTP 413. Oversized stdio lines are discarded by the transport.
Provider results have a compatibility text copy in addition to structuredContent,
so configure client transport bounds for the larger serialized MCP envelope.

HTTP sessions keep protocol metadata and require affinity across replicas.
Generation/classification never retain application conversation history. A
Platform API-key caller can explicitly use provider-managed references supported
by its API; OAuth requires full input history and `store:false`. See [OAuth](oauth.md).

Health endpoints do not call OpenAI or refresh credentials. Provider authentication
failures are returned by tool calls. `retryable:true` indicates a potentially
transient failure, not a guarantee that replay avoids duplicate provider work or
charges. Closing local HTTP I/O on cancellation does not guarantee OpenAI stopped
processing an already accepted request.
