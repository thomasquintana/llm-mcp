# Live OAuth verification follow-up

Read this plan after compaction, at task boundaries and for guidance. Run regression
tests and static analysis before committing each completed code task. Keep real
credentials out of tools/logs/git. Remove the plan after completion.

## Scope

The user has logged in. Confirm real OAuth-backed generation through MCP and fix
any demonstrated integration issue. Existing implementation and test evidence are
in docs/verification.md. Do not publish registries or push git.

## Tasks

1. [COMPLETE] Diagnose safely: auth status reports signed_in:true and no pending
   rotation. A minimal actual stdio MCP generate call reached OpenAI and returned
   HTTP 200, but the SSE collector rejected the response Content-Type. Use a
   temporary diagnostic example to inspect only response type/shape, excluding
   authorization headers and tokens. Remove that example when done.
2. [IN PROGRESS] Implement the smallest correct response-handling change based on the
   observed wire response. Add a regression demonstrating the actual failure and
   preserve bounds, completed-result semantics and error handling. Review plan and
   run format/Clippy/tests/rustdoc before committing any code task.
3. [PENDING] Repeat the minimal live MCP generation call, update durable evidence,
   complete regression/static/package gates, inspect git status/diff/log, commit
   intended changes and remove this temporary plan in a final gated cleanup commit.

## Initial evidence

- Baseline commit: 69f6771; working tree was clean.
- Real status: signed_in:true, rotation_pending:false.
- MCP live call: generate, gpt-6.1-sol, low effort, store:false, full input array.
  Error: provider_protocol, OAuth response must be an SSE stream, HTTP 200.
- Temporary MCP probe: approved temp directory /opencode/llm-mcp-live-oauth.py;
   it never prints credentials and sends one tiny generation request per run.
- Diagnosis: actual HTTP 200 response has no Content-Type header, and its bounded
  body is a genuine SSE stream with response.created/in_progress/output delta
  events. Temporary diagnostic example removed; no credentials were printed.
