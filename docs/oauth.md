# OpenAI authentication

Generation explicitly selects `LLM_MCP_GENERATE_AUTH=api_key` (default) or `oauth`.
Classification always uses `OPENAI_API_KEY`. Authentication never automatically
falls back to an API key or changes billing mode.

## Continue with ChatGPT

```sh
llm-mcp login
llm-mcp auth status
```

Login prints an authorization URL and attempts to open the system browser. It uses
OpenAI's OSS dynamic registration, PKCE, a five-minute loopback callback, state and
nonce validation, and RS256 ID-token signature/issuer/audience/expiry checks. An
issued registration is bound to one account/workspace. Log out before changing
accounts. `auth status` shows no tokens and does not contact OpenAI.

OAuth state defaults to `$HOME/.config/llm-mcp/auth.json` (`USERPROFILE` on Windows).
Set `LLM_MCP_OAUTH_CREDENTIALS_PATH` to change it. The adjacent `.host.json` keeps
stable installation identity; `.lock` enforces exclusive ownership. On Unix,
credential files must have owner-only permissions. Writes use atomic replacement
and synchronization. Stop the server before running login, import, status or logout
against its credential record. `logout` clears local tokens but retains host identity;
revoke the agent in ChatGPT settings to revoke authorization remotely.

Refresh runs before token expiry. A single refresh owner durably replaces rotating
tokens before publishing them to waiting calls. A cancelled caller does not cancel
an already-running token refresh. Failed authorization is surfaced, never retried
with different credentials. One process/replica owns each record.

## Standalone containers and Kubernetes

Complete login locally. Securely transfer the credential file and import it into
the destination's writable volume:

```sh
LLM_MCP_OAUTH_CREDENTIALS_PATH=/state/auth.json llm-mcp auth import /transfer/auth.json
```

Import retains the destination's stable host identity rather than copying the local
host ID. Only import trusted credential records. A read-only Secret mount can supply
the initial transfer file, but rotated credentials must live on writable persistent
storage. Let the destination own subsequent refreshes; do not continue using the
transferred session on the source host. The OAuth deployment uses one replica and
Recreate updates. API-key-only deployments need no OAuth volume.

## OAuth request differences

OpenAI documents ChatGPT plan use for eligible Responses requests, not Decisions.
OAuth requests must supply full history as an input array. The server internally
sets `store:false` and `stream:true`, consumes SSE through `response.completed`, and
returns the completed response object. Failed/incomplete/interrupted streams are
errors. The stream byte limit counts all events, not just the final response.

The server explicitly rejects currently documented unsupported OAuth fields:
`conversation`, `previous_response_id`, `max_output_tokens`, `max_tool_calls`,
`metadata`, `moderation`, `multi_agent`, `prompt`, `prompt_cache_retention`,
`safety_identifier`, `temperature`, `top_logprobs`, `top_p`, `truncation`, and `user`.
It also rejects `store:true`. `background:true` and `stream:true` are rejected for
both authentication modes at the MCP boundary. OAuth requires provider-compatible
function/custom namespaces and supported tools; OpenAI validates those details.

References: [registration](https://developers.openai.com/siwc/token-sharing-open-source/sign-in),
[inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference),
[limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations),
[remote hosts](https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms).
