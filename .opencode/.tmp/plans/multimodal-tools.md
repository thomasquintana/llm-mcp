# Multimodal tools implementation plan

## Working agreement

- Read this plan after every completed task, after every compaction, and whenever guidance is needed.
- Update progress and verification evidence after each task, then commit the intended changes for that task. Inspect status, diff and recent history before each commit.
- No backward compatibility: remove old configuration, code, tests and assets rather than retaining deprecated aliases or wrappers.
- Once all implementation and checks finish, remove this temporary plan in a cleanup commit and push all commits to origin.
- Do not commit credentials or runtime authentication state.

## Agreed behavior

| Tool | api_key | oauth | Input |
| --- | --- | --- | --- |
| generate | enabled | enabled | text/base64 images |
| classify | enabled | disabled | text/base64 images |
| embed | enabled | disabled | text/tokens or contextual image descriptions |
| transcribe | enabled | disabled | base64 audio |

Use only LLM_MCP_AUTH=api_key|oauth, default api_key. Serving in API-key mode requires OPENAI_API_KEY and never opens OAuth credentials. OAuth mode requires stored credentials and rejects OPENAI_API_KEY. Inactive OAuth files can remain on disk. Login/logout/status/catalog remain credential-free where appropriate. No automatic credential/billing fallback. Catalogs and runtime tool discovery match the selected mode; disabled known tools return structured tool_disabled errors before network dispatch. Keep MCP 2025-11-25 and HTLK object-root structured results.

Images: accept supported nonempty base64 image data URLs in native input_image.image_url fields. Reject remote image URLs, image file IDs/local references, invalid data/MIME and oversized media at documented image locations (messages, tool evidence, Decisions instructions). Preserve unrelated/unknown provider JSON. Validate before dispatch, retain original valid data URLs, and keep contents out of errors/logs.

Embed: standard OpenAI input string/string batch/token arrays pass to /embeddings. Alternative inputs is a nonempty batch of {content:[input_text/input_image parts]}; input and inputs are exclusive. Text-only items embed directly. Image items require description.model and allow description.instructions. Describe all images of each item together via /responses, preserving context; combine ORIGINAL context and generated detailed description with a stable delimiter for one vector per item. Cover visible text/layout/objects/relationships/retrieval details without inventing certainty. No-context images receive a general detailed description. Prepare all descriptions before one final embedding batch; fail whole call (with failing index) and skip embedding on any description failure/refusal/empty/incomplete output. Preserve batch order. Return one consistent envelope containing complete embedding response and indexed preparation text/descriptions/model/usage. Strip preparation fields from final provider request; preserve other embedding options/unknown fields. Both stages use API key.

Transcribe: file={filename,mime_type,data_base64}, model caller-selected. Base64 audio only: no URLs/paths/file IDs. Decode and upload multipart to /audio/transcriptions. Enable reqwest multipart. Encode scalars, arrays and nested options according to OpenAI form conventions; support language/prompt/keywords/languages/timestamps/logprobs/diarization/chunking/known speakers. Known speaker samples must be base64 audio data URLs and count toward limits. Default response_format=json; preserve full JSON/verbose_json/diarized_json. Wrap text/srt/vtt in an object retaining exact content/format. No model allowlist; provider validates model-specific option combinations. Reject streaming/background as with other tools.

Execution: one deadline covers queue/CPU/all pipeline stages/result building. Cancellation stops local work and subsequent stages; no nested semaphore deadlock at concurrency=1. Bound encoded input, decoded aggregate media, outbound bodies, provider responses, assembled final result. Existing request/response limits remain; document 4/3 base64 expansion (~6 MiB raw within 8 MiB default arguments). Preserve task tracking, shutdown, sanitized errors/status/request IDs, disabled retries/redirects and bounded CPU work. No media/transcript/description logging.

## Tasks and progress

0. [completed] Save plan and establish repository baseline. Commit plan before implementation.
1. [completed] Global authentication and mode-aware discovery/catalogs. Remove generate_auth/LLM_MCP_GENERATE_AUTH everywhere; correct OAuth deployments; update affected tests/tooling and changelog. Test configuration, auth, discovery/catalog parity, disabled dispatch, inactive OAuth credentials. Commit.
2. [completed] Base64 media validation and image support for generate/classify. Shared focused media helpers with meaningful invalid/oversize/reference/mixed/OAuth regressions; descriptions/schema docs and changelog. Commit.
3. [completed] Bounded provider refactor and embed pipeline. Standard and multimodal input contracts, context+description preparation, one vector/item, all-or-nothing failures and complete metadata. Tests for body/order/usage/options/failures/cancellation/limits/concurrency=1. Commit.
4. [in progress] Base64 transcription multipart upload and result decoding. Add reqwest feature/lock/notices if needed. Verify bytes/filename/MIME and scalar/array/nested fields independently in mocks, all response formats/errors/media/streaming/cancellation. Commit.
5. [pending] Complete documentation, package/CLI descriptions, mode-specific exported catalogs, HTLK workflows, integration/container tooling and deployment updates. Add real wire calls for both new tools and authentication matrix. Commit.
6. [pending] Run required final checks, fix findings, record evidence/remaining external boundaries. Commit fixes/evidence when verified.
7. [pending] Read completed plan, remove plan, commit cleanup, inspect final state and push all commits to origin.

## Required checks

- cargo fmt --all --check
- cargo clippy --all-targets --all-features --locked -- -D warnings
- cargo test --all-features --locked
- RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps --locked
- cargo package --locked --allow-dirty with fresh target directory
- cargo deny check
- python3 tools/check_notices.py
- python3 tools/export_catalogs.py --check
- uv run --with-requirements tools/requirements.txt ruff check tools
- uv run --with-requirements tools/requirements.txt python -m unittest discover -s tools -p 'test_*.py'
- cargo llvm-cov --all-features --locked --lcov --output-path target/lcov.info --fail-under-lines 70
- HTLK sibling integration, container build/smoke, Compose and kubeconform validation when installed/available. Tests use mocks, not live credentials. Regenerate notices if Cargo.lock changes. Do not claim unperformed checks or overwrite historical verification evidence.

## Baseline / blockers / decisions

- 2026-10-27: workspace clean on main at a4a8f2a. No AGENTS.md in this repository was found in initial exploration. Existing Rust/CI instructions in CONTRIBUTING.md apply.
- `git remote -v` was empty. User explicitly approved https://github.com/thomasquintana/llm-mcp.git as origin; add this remote and push there after completion.
- Existing OAuth deployments incorrectly supply OPENAI_API_KEY alongside OAuth and must be updated.
- No configuration aliases or legacy result adapters will be retained.

## Task evidence

Record each completed task's commit/checks and any follow-ups here.

- Task 0: saved the full agreed plan before implementation; verified a clean baseline and confirmed push destination with user.
- Task 1: implemented server-wide auth with no aliases, startup exclusivity, provider-side disabled checks, mode-aware descriptors and credential-free exports; removed API keys from OAuth deployments, corrected smoke tooling, added OAuth catalogs. `cargo test --all-features --locked` (36 tests), Clippy with warnings denied, formatting, catalog generation and Ruff passed. Origin added with approved GitHub URL. Remaining tool variants will be added in their implementation tasks; final tooling/deployment execution remains task 5/6.
- Task 2: added bounded canonical base64 decoding and focused image validation (messages/function/custom outputs/Decisions instructions), rejected references and unsupported MIME without leaking data, preserved unrelated JSON and OAuth image parts. Validation now runs on bounded CPU workers within the tool deadline. README/spec/descriptors/catalogs updated. All 39 Rust tests, Clippy with warnings denied, formatting and catalog generation passed.
- Task 3: added embed with standard input passthrough or prevalidated multimodal items, original context + detailed description, all descriptions before a single ordered embedding call, response/prepared_inputs envelope, usage/model/index metadata and input_index errors. Shared HTTP construction and bounded serialization now protect outbound/assembled results; pipeline holds one concurrency permit and deadline. All 46 Rust tests passed, including concurrency=1, whole-batch validation/failure, cancellation release, shared deadline, oversized prepared text/assembled output. Formatting, Clippy and catalogs passed.
