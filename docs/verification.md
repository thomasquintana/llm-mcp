# v0.1 acceptance evidence

Local verification on 2026-10-26 used macOS ARM64, Rust 1.98.0 (the declared minimum)
and Rust 1.99.1 (current stable), plus a native Linux ARM64 Docker runtime.

## Completed checks

- **30 Rust regressions** pass on both toolchains, including independent stdio and
  HTTP JSON-RPC tests. Formatting and all-target/all-feature Clippy pass with
  warnings denied. Rustdoc builds with warnings denied.
- **12 Python tooling regressions** pass, along with Ruff checks/formatting and
  actionlint 1.7.12 workflow analysis.
- LLVM line coverage is **80.37%**, above the required 70% floor. Child-process
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

Live browser authorization and real OpenAI model access require the operator's
account. No real credentials were used by the acceptance suite. Kubernetes schema
and container-volume checks are not a deployment to a live cluster. Registry
publication and the remaining native release-platform builds require repository
CI, trusted-publisher/GHCR permissions and the configured runners; they were not
performed during this local acceptance run. See [release setup](releases.md).
