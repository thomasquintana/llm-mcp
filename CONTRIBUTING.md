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
