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
2. [COMPLETE] Implement the smallest correct response-handling change based on the
   observed wire response. Add a regression demonstrating the actual failure and
   preserve bounds, completed-result semantics and error handling. Review plan and
   run format/Clippy/tests/rustdoc before committing any code task.
3. [VERIFIED; COMMIT/CLEANUP REMAINING] Repeat the minimal live MCP generation call, update durable evidence,
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
- Regression reproduced the missing-header failure before the fix. Collector now
  tolerates an absent Content-Type, matches present SSE media types correctly,
  and still rejects incompatible types, truncated/non-SSE bodies and oversized
  streams. All 31 Rust regressions, formatting, Clippy and rustdoc pass.
- After header fix, real OAuth MCP calls complete with usage.output_tokens:11,
  but the terminal response has output:[] and the expected text is missing. Need
  to inspect completed output-item/content events and assemble the full response
  like a local streaming SDK before final acceptance. Do not treat status alone
  as a successful end-to-end generation test.
- Output diagnosis: response.output_item.done contains the complete assistant
  message, including output_text and phase:final_answer. Assembler now retains
  these indexed snapshots when terminal output is empty/omitted. A regression
  failed before this fix; output ordering/function fields/incomplete snapshots
  are also covered. All 34 Rust tests and Clippy pass.
- Live end-to-end stdio MCP call now passes: model gpt-6.1-sol, completed, exact
  text "llm-mcp OAuth OK.", usage 17 input/11 output/28 total tokens. No API-key
  fallback and no further live generation calls are needed. Finish coverage,
   minimum-toolchain/package checks, durable evidence and cleanup/commit.
- Final gates passed: 34 Rust regressions and Clippy on Rust 1.98/1.99.1,
  rustdoc/format checks, 12 tooling tests/Ruff, coverage 81.70%, Cargo packaging,
  dependency/license/notice/catalog checks, actionlint and real sibling HTLK
  workflows over both transports. Rebuilt local container passes a faithful mock
  of both live OAuth behaviors and asserts generated text. Durable evidence updated
  in docs/verification.md. Commit the output assembly/evidence, then remove this
  plan in a final gated cleanup commit; no additional live requests are needed.
