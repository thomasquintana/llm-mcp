# Contributing

Use the configured stable Rust toolchain. Public APIs need rustdoc, behavior changes
need regression coverage, and changes need an Unreleased changelog entry.

Every task is gated by:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps --locked
cargo package --locked --allow-dirty
```

Use a fresh `CARGO_TARGET_DIR` for final packaging checks. Tests must run without
provider credentials. Live provider checks are opt-in. Keep credentials and runtime
authentication state out of source control.

Additional gates:

```sh
cargo deny check
python3 tools/check_notices.py
python3 tools/export_catalogs.py --check
uv run --with-requirements tools/requirements.txt ruff check tools
uv run --with-requirements tools/requirements.txt python -m unittest discover -s tools -p 'test_*.py'
cargo llvm-cov --all-features --locked --lcov --output-path target/lcov.info --fail-under-lines 70
```

Install cargo-deny 0.20.2, cargo-about 0.9.2 and cargo-llvm-cov 0.9.1 with `--locked`.
Use Python >= 3.11 and uv for tooling. Validate workflow files with actionlint 1.7.12.
When Cargo.lock changes, regenerate notices:

```sh
cargo about generate about.hbs --locked --fail --output-file THIRD_PARTY_LICENSES
```

Keep the coverage floor at least 70%; increase it as meaningful regression coverage
is added. Browser authorization and external provider account access are explicitly
opt-in. Core tests, real MCP wire tests and mock-provider tests run offline after
dependencies are available. `python3 tools/check_htlk.py` optionally exercises the
actual sibling HTLK compiler/runtime on both transports.

Container verification: build the Dockerfile, run `tools/container_smoke.py`, and
validate Kubernetes resources with kubeconform 0.7.0. See [deployment](docs/deployment.md)
and [release setup/recovery](docs/releases.md).
