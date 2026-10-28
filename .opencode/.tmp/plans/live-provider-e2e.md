# Credential-gated live OpenAI E2E tests

## Working agreement

- Read/reference this plan after every completed task, after compaction, and for general guidance. Update progress/evidence after each task and commit its intended changes.
- Inspect git status, diff and recent history before each commit. Never commit credentials or runtime auth state.
- When all tasks/checks finish, remove this temporary plan in a cleanup commit and push the completed commits to origin/main.
- No deprecated configuration, compatibility aliases, unnecessary approval gates or subagents.

## Agreed contract

Real-provider tests run AUTOMATICALLY during cargo test when credentials exist. No #[ignore] or additional opt-in flag. API and OAuth suites are gated independently. Missing credentials -> explicit skip. Present but invalid/unreadable/expired beyond recovery/locked/rejected credentials -> failure. Provider/model/rate-limit/timeout errors after attempting a suite -> failure, not skip. A wholly credential-free run remains supported and clearly reports that no live tests executed.

Live tests start the actual compiled llm-mcp binary, perform MCP initialization/discovery/tool calls over stdio AND Streamable HTTP, and connect to https://api.openai.com/v1/ (do not inherit mock OPENAI_BASE_URL). Isolate application configuration per child; never send API keys to OAuth instances. API mode: all four tools. OAuth mode: generate only, with classify/embed/transcribe locally rejected. Existing mocked contracts/wire/container/HTLK tests remain deterministic/offline.

API credential gate: nonempty OPENAI_API_KEY. OAuth gate: existing configured/default credential record (including pending recovery) using the same path resolution as server configuration; no browser login and no new raw-token path. Use the original store and refresh ownership, do not copy rotating credentials. Serialize OAuth transports so one child owns the record at a time. Parent may hold both credentials but no child mixes them.

Coverage per transport: API generation text and known base64 image; Decisions text/image predicate/choice with meaningful answer assertions; text embedding dimensions/count/indices/finite nonzero vectors; contextual multi-image embedding retaining original context and usable descriptions/index/model/usage; real spoken base64 audio transcription checking stable expected words. OAuth text/image generation, generation-only discovery and three disabled tools. Avoid exact prose/vector/punctuation assertions. Use small attributable committed image/spoken WAV fixtures; model overrides for generate/description/classify/embed/transcribe/OAuth, suitable defaults, bounded deadlines, no automatic retries. Log cases/mode/transport/model/failure code/request ID without secrets/raw media/auth-store contents. Clean child processes on success/failure.

## Tasks

0. [completed] Save plan and repository baseline; commit before implementation.
1. [completed] Add real media fixtures with provenance and a reusable independent binary/MCP harness plus credential gates/configuration/reporting. Verify offline gates, configuration isolation, bounded transport behavior and process cleanup with meaningful regressions; commit.
2. [completed] Add automatically gated real-OpenAI suites for API and OAuth over both transports with semantic assertions/model overrides. Verify available live suites and absent/invalid credentials, fix actual integration findings; commit.
3. [completed] Update CONTRIBUTING/README/testing/auth docs, changelog and permanent evidence. Run all required checks and record actual live execution vs skips; commit.
4. [in progress] Read completed plan, remove plan, commit cleanup and push all work to origin/main; verify clean/synchronized state.

## Implementation notes / decisions

- Prefer a test target that reports real skip/run/fail outcomes visibly in ordinary cargo test output (no falsely silent early-return successes). May use a custom harness=false integration target if necessary; its transport/credential helpers will have normal offline regression coverage.
- Pin only the live provider URL; do not change production authentication. Inherit needed OS/TLS/proxy settings but isolate all application settings. Keep local MCP HTTP calls off proxies.
- Do not assume a missing OAuth file when metadata access fails; permission errors fail. Never print source credential file contents. Default path resolution may use Config::from_lookup with only HOME/USERPROFILE/LLM_MCP_OAUTH_CREDENTIALS_PATH so unrelated host config cannot change the gate.
- Valid fixtures must be useful to real vision/transcription, not the tiny/dummy mock fixtures. Keep generation provenance/license documented and deterministic regeneration where possible.
- Live tests can incur provider usage, including repeated cargo test/coverage runs; user explicitly selected automatic credential gating. Use runs necessary for verification, without redundant live execution.

## Required checks

- cargo fmt --all --check
- cargo clippy --all-targets --all-features --locked -- -D warnings
- cargo test --all-features --locked (auto live gates apply)
- RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps --locked
- cargo package --locked --allow-dirty in a fresh target directory
- cargo deny check
- python3 tools/check_notices.py
- python3 tools/export_catalogs.py --check
- uv run --with-requirements tools/requirements.txt ruff check tools
- uv run --with-requirements tools/requirements.txt python -m unittest discover -s tools -p 'test_*.py'
- cargo llvm-cov --all-features --locked --lcov --output-path target/lcov.info --fail-under-lines 70
- Update notices if lockfile changes. Preserve existing historical verification claims; append current evidence and external boundaries.

## Baseline / evidence

- Clean main synchronized with origin/main at 9fa2320. Origin=https://github.com/thomasquintana/llm-mcp.git. No repository AGENTS.md found. CONTRIBUTING.md applies except its old opt-in live-test policy is superseded by the user's automatic credential gating requirement.
- No source or credential files changed before saving this plan. Availability check found no OPENAI_API_KEY, an existing default OAuth record, and no pending record; token contents were not inspected or logged.
- Task 0: saved agreed plan before implementation and inspected clean status, origin and recent commit style.
- Task 1: added tests/support/live.rs binary driver (stdio/HTTP MCP, finite time/byte limits, isolated auth, fixed live URL, no-proxy local client, cleanup and sanitized diagnostics), seven offline harness/gate/media regressions, generated 480x260 shapes/text PNG plus intelligible synthetic PCM WAV and regeneration/provenance docs. Audio generated using eSpeak NG 1.51 from pinned Debian image; no runtime fixture tools needed. `cargo test --all-features --locked --test live_harness`, Clippy with warnings denied, fmt and Ruff passed. No live provider calls yet. Custom visible-report integration runner is task 2.
- Task 2: added harness=false provider_e2e cargo target so ordinary cargo test visibly distinguishes real runs/skips; independently gates modes, pins OpenAI, uses six model overrides and timeout, implements seven API/five OAuth cases per transport plus discovery. Supports test name filters, --skip, --exact, --list, --ignored without implicit live work for unrelated selections. Ten offline driver/scenario regressions pass, including invalid credentials, semantic failures and configuration isolation. Full `cargo test --all-features --locked` passed 66 offline regressions and the REAL OAuth suite: 12 checks passed across stdio+HTTP with gpt-6.1-sol, including text/image generation and all disabled tools. OPENAI_API_KEY absent -> API suite explicitly skipped. Explicit credential-free run reported 2 skips and no live execution; --list listed suites without access. Clippy/fmt passed. No provider compatibility fixes were needed; API live behavior remains unexecuted without a key.
- Task 3: updated CONTRIBUTING/README/OAuth docs and added docs/testing.md with automatic gates, six model overrides, timeouts, standard filters, original-store ownership and provider usage. Permanent evidence appended to docs/verification.md. Required fmt/Clippy/rustdoc/cargo-deny/notices/catalog/Ruff/12 Python checks passed; 66 deterministic tests also pass on explicit Rust 1.99.1 with live suites filtered. Credential-free coverage run passed all 66 tests with 2 explicit live skips and 87.77% production line coverage (no repeated live usage). Fresh verified package: /var/folders/gb/g744mwgj3nn6mtf4dgdg58p00000gn/T/opencode/llm-mcp-live-e2e-final-package.fvWVxu. Found/reproduced Docker manifest scan failure from explicit test target; fixed .dockerignore/Dockerfile to include test/fixture/example build sources, then native Linux ARM64/Rust 1.98 build and full existing container smoke passed. Tightened nonzero stdio process-exit checking on Windows; offline harness regression/Clippy rerun passed. No dependencies changed and no native Windows or live API-key calls claimed. All implementation/verification complete; only cleanup commit and normal origin/main push remain.
