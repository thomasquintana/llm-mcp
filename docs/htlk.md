# HTLK integration

Use MCP protocol `2025-11-25`. HTLK pins complete descriptors and the exact
`llm-mcp` implementation version; regenerate catalogs when upgrading:

```sh
llm-mcp catalog --htlk --transport stdio
llm-mcp catalog --htlk --transport streamable_http
```

The exporter requires no credentials. `examples/htlk/workflow.htlk` accepts complete
generation/classification request objects and exposes full provider results. The
adjacent `requests.json` supplies generation and batched predicate/choice/score
examples. Model availability depends on the signed-in account.

For stdio, configure the HTLK deployment `llm-mcp` with the installed executable,
arguments `["serve"]`, and environment settings including `LLM_MCP_TRANSPORT=stdio`
and provider credentials. For HTTP, configure deployment `llm-mcp` with transport
`streamable_http` and URL `http://llm-mcp:8080/mcp`. Regenerate the catalog for that
transport. HTTP sessions retain MCP protocol metadata, not model conversation state.
Multiple API-key replicas require session affinity; the Kubernetes Service uses
ClientIP affinity. A restarted session must be reinitialized, not replayed blindly.

## Function continuation

1. Supply function definitions in the first `generate` call.
2. Inspect function-call output items in the returned `output` array.
3. Execute the requested function in HTLK's caller-owned workflow.
4. Call `generate` again with original context, returned call items, and matching
   `function_call_output` items containing `call_id` and the function result.

This pattern works with caller-built request objects in the same graph or a
follow-up graph; `examples/htlk/continuation.htlk` demonstrates the follow-up.
llm-mcp never executes returned functions. OAuth uses full input
history rather than `previous_response_id` and has provider-specific function-tool
requirements. See [OAuth](oauth.md).

HTLK requires object-root input/output schemas and `structuredContent`. Our output
schema preserves arbitrary provider object fields. HTLK ignores the compatibility
text copy. Its document limits include a tighter MCP raw-response allowance; set
LLM_MCP_MAX_RESPONSE_BYTES and graph output limits accordingly. Full results appear
in both text and structured content on the wire, increasing transport byte size.
HTLK also validates its canonical JSON numeric/size profile.

Tool failures carry `isError:true` and structured error details. HTLK maps these to
its MCP failure code; retry hints do not automatically authorize replay or change
HTLK policy. Use explicit workflow attempt/deadline settings appropriate for models.

Run `python3 tools/export_catalogs.py --check` to detect descriptor drift and
`python3 tools/check_htlk.py --htlk /path/to/htlk` for real compiler/runtime tests
over both transports with a local mock provider. No OpenAI credentials are needed.
