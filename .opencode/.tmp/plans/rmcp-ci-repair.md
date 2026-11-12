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

### 2. Targeted SDK upgrade and compatibility — pending

- Change Cargo.toml rmcp lower bound from 3.7 to 3.9, keeping selected features.
- `cargo update -p rmcp --precise 3.9.0`; inspect all lockfile changes.
- Regenerate THIRD_PARTY_LICENSES with cargo-about and add Unreleased changelog.
- Compile and run deterministic Rust regressions, catalog drift and audit checks.
- Make only necessary compatibility fixes, preserving protocol/exports and limits.
- Update/read this plan and commit the completed task.

### 3. Full local verification and evidence — pending

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
