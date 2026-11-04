# OpenRouter provider implementation plan

## Workflow and guidance

Read this file before each task and after every compaction. Update it with actual
results, decisions, blockers and next actions after each task; commit that update
with the task. Inspect status, diff and recent history before every commit, stage
only intended files, and never commit credentials. Finish by removing this plan,
committing cleanup, pushing the feature branch and opening a PR with `gh`.

Branch: `feat/openrouter-provider`. Base: `main` (`a9b6d2f`).

## Agreed design

- One provider per server: `LLM_MCP_PROVIDER=openai|openrouter`, default OpenAI.
- OpenRouter uses `OPENROUTER_API_KEY` and `OPENROUTER_BASE_URL` (default
  `https://openrouter.ai/api/v1/`). No key fallback or OpenAI header leakage.
- Existing OpenAI settings stay compatible. ChatGPT OAuth is OpenAI-only;
  OpenRouter plus OAuth is a configuration error. Catalogs need no credentials.
- Keep environment-only application configuration. Load `.env` only into test
  process environments, without displaying values or adding automatic dotenv.
- Reuse bounded reqwest execution: incremental body limits, bounded JSON/media,
  queue-inclusive deadlines, concurrency, cancellation and disabled retries.
- Generate: native Responses payloads/results, base64 images, full context;
  reject store:true and non-null previous_response_id for OpenRouter.
- Classify: native OpenRouter `/api/alpha/decisions` on the configured origin;
  state plus named questions object (noul/choice/score/box), optional assets and
  routing fields. Preserve complete native responses. Validate inline base64
  images and PNG/JPEG assets at native content boundaries; reject unsupported
  audio/video/file media and remote references explicitly.
- Embed: native embeddings plus existing ordered image-description pipeline,
  routed through the selected provider, preserving preparation metadata/failures.
- Transcribe: bounded multipart audio; OpenRouter json/verbose_json formats;
  JSON-encode provider/trace multipart objects. Keep OpenAI format behavior.
- Provider-aware MCP descriptors/catalogs, with existing public helpers retained
  where practical. Examples for both transports and providers.

### SDK decision

Reviewed openrouter-rs 0.24.0. It supports the four endpoints and injected reqwest
clients, but Responses requests have no catch-all map; Decisions requests omit
assets; Decisions/embedding responses discard unknown fields. Its common JSON
decoder calls response.text() before parsing, without an incremental byte bound.
Use the existing reqwest transport to preserve this server's JSON and limits.
References: https://github.com/realmorrisliu/openrouter-rs/tree/v0.24.0/src
and https://openrouter.ai/docs/llms.txt (Responses, embeddings, STT, alpha Decisions).

## Tasks and commits

- [x] 1. Save plan on feature branch. Commit: Record OpenRouter provider implementation plan.
- [x] 2. Provider enum/config, credential isolation, URL/header routing and neutral
  diagnostics. Add configuration/local HTTP regression checks. Commit: Add
  provider-aware configuration and HTTP requests.
- [x] 3. Native generation restrictions, Decisions/media validation, embedding
  pipeline and transcription formats/encoding. Local contracts and execution
  bounds regression coverage. Commit: Support OpenRouter generation decisions
  embeddings and transcription.
- [x] 4. Provider-aware tool schemas, runtime discovery, CLI exports, explicit
  catalog generation and OpenRouter HTLK examples; identity and transport checks.
  Commit: Export provider-specific MCP tools and catalogs.
- [x] 5. OpenRouter credential gate, isolated server process settings, native
  Decisions and meaningful checks for all tools over stdio/HTTP; model overrides,
  explicit missing-key skips and invalid-key/provider failures. Run mock harness
  tests and live tests with available .env credentials. Commit: Exercise
  OpenRouter through credential-gated MCP end-to-end tests.
- [ ] 6. README/config/spec/media/OAuth/testing/deployment/HTLK docs, package/CLI
  descriptions and Unreleased changelog. Run all required checks below. Record
  results/limitations. Commit: Document OpenRouter configuration and verification.
- [ ] 7. Review full branch/commits, remove plan, commit cleanup, push and open PR.
  Commit: Clean up completed OpenRouter provider plan.

## Verification

Targeted meaningful regression checks at each step. Final repository gates:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps --locked
cargo package --locked --allow-dirty
cargo deny check
python3 tools/check_notices.py
python3 tools/export_catalogs.py --check
uv run --with-requirements tools/requirements.txt ruff check tools
uv run --with-requirements tools/requirements.txt python -m unittest discover -s tools -p 'test_*.py'
cargo llvm-cov --all-features --locked --lcov --output-path target/lcov.info --fail-under-lines 70
```

Use fresh CARGO_TARGET_DIR for packaging. Regenerate notices if dependencies change.
Validate affected container/deployment artifacts as appropriate. Live tests incur
usage; never log credentials. Missing credentials skip, invalid credentials fail.

## Progress and evidence

### Task 1
- Clean working tree verified, origin is thomasquintana/llm-mcp, plan directory
  exists and .env is ignored. Feature branch created; no credentials read.
- Next: task 2, provider configuration/shared HTTP handling.

### Task 2
- Added ServiceProvider/config selection and selected-key/base URL handling;
  reject OpenRouter OAuth, isolate OpenAI headers, route alpha Decisions on the
  configured origin. Shared errors now use provider-neutral diagnostics.
- Added configuration and native HTTP regression suites, including credential
  isolation, unknown-field preservation and body-free rate-limit errors.
- Passed `cargo fmt --all` and `cargo test --locked --test configuration --test
  openrouter --test provider --lib`: 38 tests passed.
- Next: task 3, native tool contracts and media/multipart validation.

### Task 3
- Added native Decisions structural validation and shared decoded-media budget
  across state, question guidance and image assets; unsupported media fail before
  network dispatch. Native request/response fields remain intact.
- OpenRouter Responses restrictions enforced; image-description embedding pipeline
  verified on selected provider; transcription supports json/verbose_json and
  JSON multipart provider/trace options, rejecting alternate remote audio sources.
- Passed fmt and targeted lib/provider/openrouter/embed/transcribe suites (54
  tests); added a direct aggregate image/asset budget regression (55 total).
- Next: task 4, provider-specific descriptors and catalogs.

### Task 4
- Added tools_for/catalog_for/htlk_catalog_for, retaining OpenAI helper APIs.
  Runtime and CLI exports select provider; native Decisions schema and supported
  transcription formats are advertised. ChatGPT auth commands reject OpenRouter.
- Added generated OpenRouter catalogs for stdio/HTTP, with explicit provider
  selection in generator. Existing OpenAI catalogs remain byte-for-byte stable.
- Passed fmt, identity + independent transport suites (10 tests, discovery covers
  both providers), and catalog regeneration/drift checks.
- Next: task 5, isolated OpenRouter live suites and credentials from .env.

### Task 5
- Added OpenRouter credential gate and model overrides, native classification
  semantic checks, shared process isolation and real function-call continuation.
  Mock driver checks both providers over both transports and rejects invalid keys.
- Initial live checks found typesafe/jev-latest nonexistent; typesafe/jev-1.14
  works. Jev rejects the original rectangular PNG dimensions; generated a 512x512
  version of the same synthetic image (existing fixtures unchanged). Native state
  text is plain strings, not Chat Completions text parts.
- Passed live OpenRouter suite with .env key: 18 checks across stdio and HTTP,
  including discovery, text/image generation, text/image native Decisions,
  text/image-prepared embeddings, audio transcription and function continuation.
- Passed offline harness (10 tests) and live-suite --list (three suites).
- Next: task 6, complete docs and final repository gates.
