# v0.1 acceptance evidence

## rmcp security-advisory CI repair

The guide merge's [main CI run](https://github.com/thomasquintana/llm-mcp/actions/runs/43119190461)
failed only the dependency-policy job on RUSTSEC-2026-0462 for rmcp 3.7.0.
The targeted update raises the SDK requirement to 3.9 and locks 3.9.0, the first
patched release. No other package versions changed; generated notices only update
the rmcp version label. No advisory exceptions or application changes were needed.

Local verification on macOS ARM64 with Rust **1.98.0**:

- `cargo deny check` passes all four categories; existing duplicate-version and
  unused-license-allowance warnings remain. Generated notices and all six
  provider/auth/transport catalog checks pass; MCP remains **2025-11-25**.
- **77 deterministic Rust regressions** pass, including independent stdio/HTTP
  contracts, cancellation, shutdown, payload bounds and OAuth isolation. All three
  live suites were explicitly filtered out. Formatting, warnings-denied all-target
  Clippy and rustdoc pass; production line coverage is **88.67%**.
- Clean `cargo package --locked` verification compiles the staged crate in a fresh
  target directory. Ruff, **12 Python tooling tests**, and actionlint pass.
- The actual Linux ARM64 container builds with the pinned Rust 1.98 toolchain and
  passes API-key tools, OAuth-only generation, graceful termination, credential
  import and remount permission-repair smoke checks. Both Compose configurations
  validate; pinned kubeconform 0.7.0 validates all six Kubernetes resources.
- The sibling HTLK compiler/runtime passes the four-tool workflow and function
  continuation over both transports with local mock providers.

No real provider credentials or live cluster deployment were used for this repair.
All seven jobs pass in [repair PR #11](https://github.com/thomasquintana/llm-mcp/pull/11)
[CI run 43135698022](https://github.com/thomasquintana/llm-mcp/actions/runs/43135698022)
for `3ba04da`: Linux/macOS/Windows Rust checks, Rust 1.98 MSRV, dependency policy
and clean packaging, deployment/container smoke checks, and coverage.

## OpenRouter CI repair verification

All seven jobs pass in [CI run 41953459099](https://github.com/thomasquintana/llm-mcp/actions/runs/41953459099)
for repair commit `d89cda0`: Rust checks on Linux, macOS and native Windows, MSRV,
policy (including clean packaging and workflow analysis), deployment/container
smoke tests, and coverage.

- CI explicitly enables cargo-about 0.9.2's `cli` feature; a fresh local install
  also produced the executable and passed notice generation/comparison.
- OAuth live-test discovery validates directory ancestors before interpreting
  missing files. A credential path beneath a regular file fails on Windows rather
  than being silently skipped. Missing nested directories still skip without
  creating filesystem state; diagnostics exclude credential paths.
- The original regression remains active on every platform. Local formatting,
  Clippy with warnings denied, 77 deterministic Rust tests, warnings-denied rustdoc,
  fresh-target packaging, catalogs, Ruff, 12 Python tests and actionlint pass.
  Live-provider requests were not repeated for these CI/test-harness-only fixes.

## OpenRouter provider update — 2026-11-04

Verified on macOS ARM64 with Rust **1.98.0**:

- **77 deterministic Rust regressions** pass, including native OpenRouter HTTP
  routing, key/header isolation, payload preservation, classification media/assets,
  request/response bounds, multipart JSON options, image-description embeddings,
  provider-aware discovery and independent stdio/HTTP process checks.
- The real OpenRouter API-key suite passes **18 checks** over stdio and HTTP:
  discovery, text/image generation, text/image native alpha Decisions, text and
  image-description embeddings, spoken-audio transcription and caller-executed
  function continuation. Models used were `openai/gpt-4.1-mini`,
  `typesafe/jev-1.14`, `openai/text-embedding-3-small` and
  `openai/whisper-large-v3`. Credentials were loaded from ignored `.env` only for
  this explicit live run.
- Live verification found the bare `typesafe/jev-latest` model ID unavailable and
  the original 480×260 image rejected by Jev. Tests now use a reproducible 512×512
  version of the same shapes and `typesafe/jev-1.14`.
- The ordinary full test run also passes **12 real OpenAI OAuth checks** over both
  transports. OpenAI API-key access was absent from that process and explicitly
  skipped; its live API-key suite was not executed in this update.
- Formatting, all-target/all-feature Clippy with warnings denied, warnings-denied
  rustdoc, dependency policy, generated notices and six provider/auth/transport
  catalog drift checks pass. Dependency policy reports existing duplicate-version
  and unused-license-allowance warnings; no dependencies or exceptions were added.
- **12 Python tooling tests** and Ruff pass. Production LLVM line coverage is
  **88.67%**, measured with all live credentials explicitly unavailable; all three
  live suites report skips in that run. The native OpenRouter validation module
  reaches **98.44%** line coverage.
- Cargo packaging succeeds with independent compilation in a fresh target
  directory. Real sibling HTLK default-provider workflows and continuation checks
  pass over both transports against local providers.
- Both Compose configurations validate, and containerized kubeconform **0.7.0**
  validates all six Kubernetes resources. Building the current Docker image was
  blocked by a timeout fetching the pinned Dockerfile frontend; a legacy-builder
  fallback also timed out. The subsequent [CI deployment job](https://github.com/thomasquintana/llm-mcp/actions/runs/41887225966/job/126774820934)
  successfully built the image and passed container smoke tests, resolving this
  local verification limitation. Existing deployment manifests and Dockerfile are unchanged.

Provider-specific contracts, native alpha classification and usage/model overrides
are documented in [OpenRouter](openrouter.md) and [testing](testing.md).

## Automatic live-provider E2E update — 2026-10-28

- An ordinary `cargo test --all-features --locked` run passed **66 deterministic
  Rust regressions** and automatically ran the available real OAuth suite against
  `https://api.openai.com/v1/`, using `gpt-6.1-sol`.
- The live OAuth suite passed **12 checks**: discovery, text generation, base64
  image interpretation, and local rejection of classification/embedding/
  transcription, over **both stdio and HTTP**. Four generation requests reached
  OpenAI through actual `llm-mcp` child processes. Image assertions verified the
  red square, blue circle and printed number in the committed fixture.
- `OPENAI_API_KEY` was absent, so the API-key suite explicitly reported a skip.
  Its live classification, embedding and audio-transcription scenarios are
  implemented, but were **not executed against OpenAI** in this update.
- A credential-free run reported two skipped suites and “No live OpenAI tests
  executed.” Suite listing and standard test filters were verified without live
  calls. Ten offline regressions cover credential gates, present-but-invalid
  OAuth state, API authentication rejection, auth isolation, process cleanup,
  semantic assertions, result redaction and fixture validity.
- Formatting, all-target/all-feature Clippy with warnings denied, warnings-denied
  rustdoc, dependency policy, generated notices and catalog checks pass. The
  deterministic suite also passes on explicit Rust **1.99.1**, with live suites
  filtered out to avoid repeating provider usage. The primary run used **1.98.0**.
- **12 Python tooling tests** and Ruff pass. LLVM production line coverage is
  **87.77%**, measured in an explicitly credential-free environment so coverage
  does not depend on external service access.
- Packaging succeeds with independent compilation in a fresh target directory.
  The explicit Cargo test target required adding test/fixture/example sources to
  the Docker build context; the corrected Linux ARM64/Rust 1.98 container builds
  and passes the existing mock-backed API-key/OAuth smoke tests.

Tests now activate real service calls automatically when the corresponding
credentials exist; this replaces the former opt-in live-test policy. The
[testing guide](testing.md) documents independent gates, model overrides, filters,
credential ownership, usage and skip/failure reporting. No new native Windows
execution or live API-key-provider validation is claimed. Earlier evidence below
is historical and retains its original execution boundaries.

## Multimodal tools update — 2026-10-28

Verified locally on macOS ARM64, with a native Linux ARM64 Docker runtime:

- **56 Rust tests** pass on Rust **1.98.0** (the declared minimum) and **1.99.1**.
  All-target/all-feature Clippy passes with warnings denied on both toolchains.
  Formatting and rustdoc with warnings denied pass.
- LLVM line coverage is **87.38%**. Media validation and the new embedding and
  transcription modules each reach at least 95% line coverage.
- **12 Python tooling tests**, Ruff, and actionlint **1.7.12** pass.
- `cargo deny check` passes advisory/source/license/bans policy with the existing
  transitive-duplicate and unused-license-allowance warnings. Generated notices
  match the updated lockfile, and all four mode/transport catalogs pass drift checks.
- Cargo packaging succeeds in a fresh target directory, including independent
  compilation of the staged crate.
- Independent stdio and Streamable HTTP JSON-RPC tests exercise all four API-key
  tools with base64 image/audio input and verify returned data. A Unix stdio OAuth
  test verifies image generation, generation-only discovery, and rejection of all
  three disabled tools.
- Provider regressions independently parse multipart uploads and verify exact file
  bytes, filename, MIME type, repeated/nested options and known-speaker samples.
  All six transcription result formats, malformed input, request/result bounds,
  cancellation, deadlines and sanitized failures are covered.
- Embedding regressions verify shared-context multi-image descriptions, original
  context plus description as the exact embedding text, batch ordering, indexed
  preparation/model/usage metadata, whole-call failure, and no embedding request
  after a preparation failure. Pipelines work with concurrency one and share one
  end-to-end deadline. Computer screenshot evidence follows the same base64 rule.
- The actual sibling HTLK compiler/runtime successfully executes the four-tool
  workflow and function continuation over both transports against local providers.
- The updated nonroot container builds using Rust **1.98.0** and passes API-key
  generation/classification/embedding/transcription, OAuth-only generation and
  disabled-tool checks, graceful shutdown, persistent identity, credential import
  and remount permission repair. Both Compose configurations validate, and
  kubeconform **0.7.0** strictly validates all six Kubernetes resources.

These checks use mock provider responses and credentials. Live Platform image
interpretation, embedding quality and audio transcription were not executed in
this update. Windows-native behavior, other release targets, live Kubernetes
deployment and registry/crates.io publication remain external CI/operator checks.
The earlier acceptance evidence below is retained as historical evidence.

Local verification on 2026-10-26 used macOS ARM64, Rust 1.98.0 (the declared minimum)
and Rust 1.99.1 (current stable), plus a native Linux ARM64 Docker runtime.

## Completed checks

- **34 Rust regressions** pass on both toolchains, including independent stdio and
  HTTP JSON-RPC tests. Formatting and all-target/all-feature Clippy pass with
  warnings denied. Rustdoc builds with warnings denied.
- **12 Python tooling regressions** pass, along with Ruff checks/formatting and
  actionlint 1.7.12 workflow analysis.
- LLVM line coverage is **81.70%**, above the required 70% floor. Child-process
  instrumentation is preserved without inheriting application configuration.
- Dependency advisory/source/license policy, generated notice comparison and
  exported HTLK catalog drift checks pass. JWT validation uses AWS-LC; no advisory
  exceptions were introduced.
- Cargo packaging was verified offline in a fresh target directory, independently
  rebuilding the staged crate. Offline mode avoids transient registry-index access;
  it does not skip package compilation or verification.
- The actual sibling HTLK compiler/runtime successfully executes generation,
  batched classification and function continuation over both supported transports
  using a local provider. `tools/check_htlk.py` reproduces these four workflows.
- The standalone nonroot container passes API-key generation, OAuth generation
  with internal SSE, API-key classification, graceful termination, persistent
  identity, trusted credential import and group-permission remount/repair checks.
- Both Compose configurations validate. Kubeconform strictly validates all six
  Kubernetes resources. The example owner-run permission-repair command is also
  exercised against a real container volume.
- The exported Linux ARM64 binary is byte-identical to the executable extracted
  from the actual image layers. Verification handles compressed and uncompressed
  Docker layers and containerd OCI index identities.
- The Windows ACL module and its Windows-only permission test cross-type-check
  and pass Clippy against `x86_64-pc-windows-gnu`; native ACL execution is assigned
  to the Windows CI job.

## Important regression boundaries

Tests cover complete request/response preservation, unknown provider fields,
function history and output schemas, explicit streaming/background rejection,
deadline and cancellation behavior, concurrency release, body/stream bounds,
malformed/truncated/failed SSE, provider request-ID retention, and HTTP/2 protocol
NACKs without retry even when dependency features enable HTTP/2.

OAuth tests cover PKCE, callback state, required JWT claims, issuer/audience/nonce,
signatures and authorized party; exclusive ownership/import; shared refresh
success/failure; cancellation and shutdown during rotation; and durable pending
recovery after persistence or JWKS failures without reusing a consumed token.

Release regressions cover strict version/tag matching, archive licenses/checksums,
deterministic archives, unchanged published-crate detection, original artifact-set
reuse, image/binary identity binding, registry failures, and interrupted version or
commit alias publication without replacement.

## External execution

After operator-completed login, a real stdio MCP `generate` request using ChatGPT
OAuth and `gpt-6.1-sol` completed with the exact output `llm-mcp OAuth OK.`. That
request used 17 input tokens and 11 output tokens, with no API-key fallback.

The live check exposed two provider behaviors now covered by regressions: an
omitted SSE Content-Type and an empty terminal output snapshot despite complete
output-item events. The collector accepts the missing header, assembles completed
output items in index order, and still rejects truncated, oversized, incomplete
or incompatible streams. The container mock now reproduces these live wire shapes
and asserts actual generated text rather than merely a successful result object.

The automated acceptance suite continues to use mock credentials; live API-key
generation/classification and browser UI testing require separate operator checks.
Kubernetes schema and container-volume checks are not a deployment to a live cluster. Registry
publication and the remaining native release-platform builds require repository
CI, trusted-publisher/GHCR permissions and the configured runners; they were not
performed during this local acceptance run. See [release setup](releases.md).
