# rmcp security-audit CI repair plan

## Goal and working rules

Restore all seven CI jobs by upgrading the vulnerable locked rmcp SDK, retaining
the project's MCP 2025-11-25 contracts, behavior, MSRV, limits and auth separation.
User explicitly selected the CI repair rather than another user-guide revision.

- Branch: `fix/rmcp-security-audit`, based on main `c849060` (merged guide PR #10).
- Read this plan before work, after every task, after compaction, and for guidance.
- Update outcomes/checks after every completed task and commit that task's changes.
- Before every commit inspect status, diff and recent history; stage intended files.
- Review all branch commits/base diff, push and open a PR, then verify all seven
  CI jobs. Record the PR and execution evidence and commit/push the final update.
- Preserve unrelated work and credentials. Use deterministic test filters, mock
  providers and isolated credentials; do not source .env or open real OAuth state.
- Do not suppress the advisory or weaken policy. Avoid unrelated dependency updates.

## Confirmed diagnosis

Main run https://github.com/thomasquintana/llm-mcp/actions/runs/43119190461 fails only
`checks / policy` at `cargo deny --config deny.toml check`: RUSTSEC-2026-0462,
unbounded response-body buffering in rmcp Streamable HTTP clients, patched >=3.9.0.
Linux/macOS/Windows Rust jobs, MSRV, coverage and deployment passed. The lockfile
was unchanged by the guide merge and the same audit failure existed in PR #10.
Application enables server transports, not the affected built-in client transports;
package audit still fails. Upgrade to the first patched release without exceptions.

Reviewed upstream 3.7.0..3.9.0 diff/release notes. Rust minimum remains 1.88,
compatible with this project's 1.98. Relevant changes include HTTP media-type
parsing, stateful session cancellation and protocol error replies. Existing wire
tests cover main contracts; add regressions only if a concrete gap/failure emerges.

## Tasks

### 1. Branch and saved plan — completed

Fetched current main, confirmed clean workspace, created the branch from c849060.
Reviewed CONTRIBUTING and prior guide verification. Save/commit this plan.

### 2. Targeted SDK upgrade and compatibility — completed

Raised requirement to 3.9 and resolved exactly 3.9.0. No unrelated package version
changes; rmcp adds a sha2 dependency already in this graph. Regenerated notices
(only rmcp version label changed), added changelog entry. No application source or
catalog changes required. All 77 deterministic Rust tests pass on Rust 1.98 with
three live suites filtered out. `cargo deny check` passes (existing license and
duplicate-version warnings only); notices and six catalog drift checks pass.

- Change Cargo.toml rmcp lower bound from 3.7 to 3.9, keeping selected features.
- `cargo update -p rmcp --precise 3.9.0`; inspect all lockfile changes.
- Regenerate THIRD_PARTY_LICENSES with cargo-about and add Unreleased changelog.
- Compile and run deterministic Rust regressions, catalog drift and audit checks.
- Make only necessary compatibility fixes, preserving protocol/exports and limits.
- Update/read this plan and commit the completed task.

### 3. Full local verification and evidence — completed

Passed remaining CONTRIBUTING checks on macOS ARM64 / Rust 1.98.0: fmt, all-target
all-feature denied-warning Clippy, denied-warning rustdoc, clean fresh-target cargo
package (81 files), coverage with live suites filtered out (88.67% lines), Ruff,
12 Python tooling tests and actionlint. `cargo deny --log-level error check`
explicitly confirms advisories/bans/licenses/sources all OK. Notices and catalogs
were already checked after the targeted update; no new dependency/source changes.

Built `llm-mcp:rmcp-audit-repair` with the unchanged Dockerfile/pinned Rust 1.98.
Registry metadata lookups took several minutes; combined build+smoke hit the
600-second wrapper deadline after the smoke success message. Standalone rerun
`python3 tools/container_smoke.py --image llm-mcp:rmcp-audit-repair` exited cleanly
with all assertions passing; no test containers/volumes remain. Both Compose
configs validated with `--env-file /dev/null` and a dummy key. Pinned kubeconform
0.7.0 validated all six resources. `tools/check_htlk.py` passed the actual sibling
HTLK workflow and continuation over both transports with sanitized auth/provider
environment and local mock providers. Added current evidence in docs/verification.md.
No new regression tests/source fixes were needed; existing coverage is sufficient.

- Formatting, all-target/all-feature denied-warning Clippy, deterministic tests,
  denied-warning rustdoc, fresh-target independent cargo package verification.
- Audit, notices, six exported catalog checks, Ruff, 12 Python tooling tests.
- Coverage >=70% with live suites filtered out; MSRV is Rust 1.98 locally.
- Build actual Docker image and run tools/container_smoke.py. Validate Compose and
  Kubernetes resources if available. Exercise sibling HTLK integration if available.
- Record exact outcomes/environment limitations in docs/verification.md and plan.
- Read/update plan and commit verification/documentation changes.

### 4. Final review, PR and CI — pending

- Review complete diff, all commits, clean status, tracking and secrets/artifacts.
- Push and open repair PR to main; monitor all seven jobs to completion.
- Fix any failures with plan updates and separate commits, then verify fresh CI.
- Record PR URL and passing CI run in docs/verification.md and this plan, commit,
  push and ensure the final revision has passing CI and clean synchronized status.

## Execution evidence

- Task 1 complete: starting targeted dependency update after this plan commit.
- Task 2 complete: reread plan for alignment. The advisory is resolved without
  policy exceptions, wire contracts unchanged. Next run the remaining required
  checks and actual container/integration verification before opening the PR.
- Task 3 complete: reread plan. All planned local checks passed, including actual
  container and HTLK execution. Next commit evidence, review every branch commit
  and base diff, open the PR, and wait for all seven CI jobs to pass.
