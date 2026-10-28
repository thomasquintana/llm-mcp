# v0.1 acceptance evidence

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
