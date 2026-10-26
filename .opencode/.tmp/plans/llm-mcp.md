# llm-mcp implementation plan

## Working agreement

Read this plan before each task, after every compaction, and whenever guidance is needed. After completing a task, run regression tests and static analysis, record results here, review the plan, and commit only the intended files. A task is not complete until its gates pass. Do not push or publish without a separate request. Remove this temporary plan only after all tasks and final verification are complete. Keep lasting design and operating instructions in docs.

## Accepted specification

- Rust 2024/latest stable; Apache-2.0; one crate containing a reusable library and CLI, with focused modules and documented public APIs. Follow sibling htlk quality/release conventions without copying its unrelated multi-crate architecture.
- Tools are `generate` (OpenAI Responses) and `classify` (OpenAI Decisions). No separate reason tool. Reasoning effort is a request option.
- Full provider-compatible conversation input, including tool calls and outputs; generation can return new function/custom call requests, executed by the caller. Support text, structured output schemas and other provider request fields. Pass through unknown request/response fields. Server connection settings are not callable arguments.
- Both tools return full provider JSON objects as MCP structuredContent; include protocol-valid content. Stable object-root input/output schemas; reproduce descriptors for htlk catalogs. htlk requires protocol 2025-11-25 and exact server identity/descriptor matches.
- Stateless application conversations. Explicitly reject stream:true and background:true at the callable boundary. Foreground completed results only. OAuth Responses requires internal SSE consumption, store:false and stream:true; these internal requirements do not expose streaming to clients. Preserve completed response objects; fail on incomplete/interrupted/failed streams.
- classify accepts a shared input and multiple named predicate/choice/score questions. Preserve probabilities/confidence and per-question refusals. Caller owns thresholds, workflows and escalation.
- Environment-only application configuration. No TOML, profiles or configuration merging. Config includes transport/bind address, OpenAI base URL/key/org/project, explicit generation auth selection, OAuth credential path, deadlines, concurrency and size limits.
- generate auth is explicitly selected API key or Sign in with ChatGPT OAuth. classify initially always uses an API key. No automatic billing/auth fallback. Support OpenAI-documented OSS public-client OAuth flow with browser/loopback PKCE, state/nonce and ID-token validation, host IDs, token persistence and automatic refresh. OAuth capabilities differ from Platform API-key capabilities; report incompatible request fields rather than silently dropping them.
- CLI: serve, login, logout, auth status, credential import and catalog export. OAuth state is separate from environment configuration. One account per instance. Protected files, atomic writes and coordinated refresh. Local login then import to remote host preserving destination host ID. One process/replica owns a credential record.
- Stdio and Streamable HTTP selectable per process. HTTP MCP authentication is future work. Health probes for HTTP. Stdout is protocol-only in stdio; logs on stderr, without request contents or credentials.
- Configurable deadlines, cancellation propagation, concurrency and request/result byte bounds. No provider retries by default. Structured errors preserve retryability and provider request ID without replaying calls. Graceful shutdown.
- Same nonroot linux/amd64 + linux/arm64 container for standalone Docker/Compose or K8s. API-key deployments may scale; OAuth deployments use one replica, Recreate strategy and writable persistent volume. Env secrets injection, probes, resources and termination examples.
- Distribution: crates.io plus automated Linux x86_64/ARM64, macOS x86_64/ARM64 and Windows x86_64 binaries, checksum/license archives, GHCR multiarch images. Version-tag validation, exact-commit gates, release dry run, narrowly scoped publishing jobs and resumability. Publishing itself is not requested.

## Architecture and dependencies

One package, lib.rs and main.rs. config.rs/error.rs; provider/ for HTTP and SSE; auth/ for OAuth/store; mcp/ for descriptors and handlers; runtime/ for transport and lifecycle. Tokio, rmcp (explicit 2025-11-25 compatibility), Axum, reqwest/rustls, serde_json, tracing, clap, OAuth/JWT primitives. Keep dependencies focused and Cargo.lock committed. Public constructors allow mock endpoints and deterministic regression tests without real provider credentials.

## Task 1 — foundation [COMPLETE]

1. Persist this complete plan and initialize git (directory is empty, not a repository).
2. Add package metadata, stable toolchain, library/CLI skeleton, Apache license and gitignore (including runtime credentials).
3. Add lasting specification and contributor instructions with format/clippy/test/rustdoc/package gates.
4. Establish executable static-analysis/regression commands and bootstrap test.
5. Run gates, review status/diff/history, update this plan, commit foundation.

## Task 2 — request/config/provider/tools [IN PROGRESS]

1. Parse environment settings without mutating global environment in tests. Validate positive limits and explicit auth, redact secrets. Request model is caller supplied.
2. Implement bounded reqwest API-key HTTP calls, pass-through bodies, raw object responses, provider status/error mapping, no retries/redirects, org/project headers.
3. Reject streaming/background and nonobject payloads. Preserve future fields. Define stable MCP schemas and output descriptors, protocol compatibility and catalog export.
4. Add core concurrency/deadline/cancellation handling and MCP handlers. Expose structuredContent and protocol content consistently.
5. Mock-provider tests: unknown fields, full history, function call output, schema settings, refusal batches, HTTP auth/headers, rate limits/timeouts/malformed/oversized bodies and option rejection before dispatch.
6. Run gates, update plan, commit.

## Task 3 — OAuth lifecycle [PENDING]

1. Implement protected credential records and host identity, atomic rotation, exclusive process ownership and safe import/logout/status.
2. Implement documented OpenAI dynamic OSS registration and PKCE loopback login, browser URL, state/nonce validation, OIDC discovery/JWKS signature/issuer/audience/expiry validation, explicit granted scopes and token exchange.
3. Refresh tokens before expiry with coordination, persist all rotated values atomically, fail explicitly on revoked/invalid sessions. No API-key fallback.
4. Route OAuth generation using internal bounded SSE, enforce/document OAuth field support and terminal-event behavior.
5. Tests for PKCE/callback failures, token validation, import/host preservation, refresh single-flight/rotation/restart, credential permissions, and SSE splits/failure/incomplete/truncation.
6. Run gates, update plan, commit.

## Task 4 — transports and htlk [PENDING]

1. Wire serve stdio and Streamable HTTP with protocol 2025-11-25, JSON responses where supported, health endpoints, signal shutdown and cancellation.
2. Provide htlk catalog exporter, pinned generated catalogs and example source/endpoint configurations for generation, function continuation and classification.
3. Independent wire tests for initialize/list/call/cancel, stdout discipline, HTTP sessions/statelessness, health, graceful shutdown and both auth dispatch paths.
4. Verify actual htlk integration where local sibling permits; record exact compatibility and response-size guidance in docs.
5. Run gates, update plan, commit.

## Task 5 — containers and deployment [PENDING]

1. Add Dockerfile, dockerignore, Compose and standalone Docker examples using same image for HTTP/stdio.
2. Add plain K8s Deployment/Service/probe/resource/Secret references, OAuth PVC/Recreate/single-replica example and host-aware credential import procedure.
3. Build/run image and mock provider smoke tests when Docker available; validate manifests and shell scripts. Verify nonroot writable credential volume and host bindings.
4. Run regression/static gates plus deployment checks; update plan; commit.

## Task 6 — CI, releases and durable docs [PENDING]

1. CI stable Linux/macOS/Windows quality+tests; format/Clippy/rustdoc/package; locked deps; dependency/license policy and generated third-party notices; coverage evidence/ratchet and workflow lint.
2. Tag/manual dry-run release validates exact version/commit and runs all gates. Build/test archives for five targets with checksums/notices. Use musl Linux if dependency/toolchain checks pass, otherwise documented baseline.
3. Publish crates.io, GHCR multiarch image and GitHub release in dedicated least-privilege jobs; trusted publishing if possible; draft/finalize release, immutable version tags and resume safeguards.
4. Finish README/API/config/OAuth/deployment/contribution/release docs, changelog, license inventory and Dependabot.
5. Validate workflows, package, docs and deployment artifacts, run all regression/static gates, update plan, commit.

## Task 7 — final acceptance and cleanup [PENDING]

1. Review accepted specification against implementation; fix gaps with regression coverage.
2. Run complete regression/static/docs/package/license/workflow/deployment gates. Any unavailable external infrastructure must be documented honestly and addressed with offline/CI coverage.
3. Verify clean git and intended task commits. Move lasting evidence/limitations into docs.
4. Read this plan, mark outcomes, remove temporary plan, run applicable gates, review status/diff/history, commit final cleanup. Report completed behavior, verification and any external setup needed for releases.

## Required gate commands

`cargo fmt --all --check`
`cargo clippy --all-targets --all-features --locked -- -D warnings`
`cargo test --all-features --locked`
`RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps --locked`
`cargo package --locked --allow-dirty` (fresh target directory for final/release checks)
Additional policy/deployment/workflow checks are added as their artifacts are introduced. Do not mark a task complete on intent alone.

## Evidence / decisions

- 2026-10-26: Project directory confirmed empty; Rust/Cargo 1.98 available; Docker CLI and gh available. Git initialization required. OpenAI OAuth docs require stream:true/store:false internally; Decisions OAuth is undocumented, so classify uses API key.
- htlk research: exact MCP 2025-11-25, required outputSchema and object structuredContent, descriptor digests include descriptions/annotations. Protocol descriptors must remain deterministic. htlk raw JSON has safe-number/size constraints and no success text fallback.
- Release research: htlk baseline has strong Rust/package/license gates but binary/container delivery must be added. Never publish just because release workflow is implemented.
- Task 1 complete: git initialized on main; specification/package/toolchain/license added. Format, Clippy, regression test, warning-free rustdoc and verified Cargo packaging all passed. Plan reviewed before foundation commit.
