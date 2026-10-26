# Releases

The Release workflow supports validation-only manual runs (dry run defaults true)
and publication on strict `vMAJOR.MINOR.PATCH` tags. Publishing requires the tag to
match Cargo.toml and the tagged commit to be reachable from `origin/main`.

## Outputs and gates

Every release validates the exact commit with the reusable Checks workflow:
formatting, Clippy/rustdoc warnings-as-errors, Linux/macOS/Windows tests, explicit
Rust 1.98 MSRV tests, dependency
advisories/licenses, notice/catalog drift, verified clean packaging, Python tooling
tests/lint, workflow analysis, Kubernetes schemas and standalone image smoke tests.
Linux line coverage has a 70% floor. Coverage and rustdoc artifacts are retained.

Five native binary archives ship with README, Apache license, full third-party
notices, and individual SHA-256 checksum files:

- `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` (glibc >= 2.36).
- `x86_64-apple-darwin` and `aarch64-apple-darwin`.
- `x86_64-pc-windows-msvc`.

Linux binaries come from the same tested Debian bookworm image builds that supply
the OCI image. Linux architectures build natively, avoiding cross-compiling native
TLS dependencies. Rust 1.98 is the minimum tested toolchain and pinned container
builder; development/CI also follow latest stable. macOS and Windows binaries are
built/tested on native runners; macOS archives are unsigned.

After validation and builds, a preparation job freezes one artifact set in a draft
GitHub Release. `receipt.json` binds the source commit, every archive/checksum/crate,
native image config IDs and the hash of each image's actual server executable. It
verifies that Linux binary archives contain the same executable as the tested images.
Only then do dedicated publishing jobs release:

1. `llm-mcp` to crates.io using OIDC trusted publishing.
2. The tested native images to GHCR with immutable version and commit tags, then a
   combined amd64/arm64 image index.
3. Finalize the prepared GitHub Release after both package channels succeed.

The release retains native image tar archives, the crate archive and the receipt as
well as standalone binary archives/checksums, allowing exact artifact-set recovery.

Images are tagged `ghcr.io/OWNER/llm-mcp:VERSION` and `:sha-COMMIT`. Use a version or
digest in deployment configuration. No moving `latest` tag is required.

## Repository setup

Create the GitHub repository and configure `crates-io` and `releases` environments.
Authorize `release.yml` as a crates.io trusted publisher for this repository and
the `crates-io` environment. Reserve/bootstrap crate ownership if required by the
registry before the first automated publication. Allow GITHUB_TOKEN to publish
the repository's GHCR package and release assets. Publishing credentials are
available only to publishing jobs; ordinary PR jobs have read-only permissions.

For each release, update Cargo version, changelog and versioned deployment examples,
regenerate catalogs and notices, commit the release contents, and run a dry run.
Then create the matching annotated tag and push it when ready to publish. Installing
these workflows does not itself publish anything.

## Recovery

Publication across registries is not atomic. Prefer **re-running failed jobs** so
the successful build artifacts are reused. Once a receipt exists, preparation
downloads the original frozen files even if a full rerun rebuilt different inputs.
If preparation was interrupted before receipt creation, resume using the original
successful build artifacts; conflicting partial assets fail rather than being
overwritten. Artifacts and preparation outputs retain their original attempt IDs.
An existing crate is accepted only if
its checksum matches the locally packaged crate. An existing container version
must match its receipt's native config IDs and immutable commit-tag digest. A
version-only or commit-only index publication is repaired by creating just the
missing alias to the verified existing digest. Existing GitHub assets are downloaded
and compared rather than overwritten. Changed release contents require a new
version; do not move an existing release tag.

Artifacts built on another OS and actual registry publication require their CI
runners and repository permissions. Local tests use mock OpenAI endpoints and do
not consume provider credentials. Real browser login and model-access checks are
operator-initiated integration checks.
