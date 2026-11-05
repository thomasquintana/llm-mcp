# PR #7 CI repair plan

Read before each task and after compaction. Record verification/results before
committing each completed task; stage only intended files. Continue on
feat/openrouter-provider and push fixes to existing PR #7. Remove this plan in a
cleanup commit after verification; monitor the final pushed commit's CI as well.

## Diagnosis

Run 41887225966 has two failures:
- Policy: cargo-about 0.9.2 installs no executable without --features cli; notice
  checks fail with no such command: about. Default features do not enable cli.
- Windows: querying a path below a regular file reports NotFound rather than
  NotADirectory. The OAuth live gate incorrectly skips invalid configuration.
  Keep the existing cross-platform regression and distinguish missing directories
  from non-directory ancestors in the gate.

Other jobs passed, including deployment/container smoke, macOS/Linux, MSRV and
coverage. Update permanent verification evidence to reflect CI deployment success.

## Tasks

- [x] 1. Confirm clean branch and job failures; save and commit plan.
- [x] 2. Enable cargo-about cli in workflow and contributor instructions. Validate
  OAuth credential ancestors portably, retaining fail-vs-skip behavior and adding
  missing nested-path/deep invalid-path coverage. Run targeted tests, formatting,
  Clippy, full deterministic tests, rustdoc, notice/catalog/Python checks, workflow
  analysis, fresh installation/package checks as appropriate. Update changelog
  and verification evidence. Commit implementation and this plan update.
- [ ] 3. Push fixes and monitor CI; diagnose/fix any further failures without
  weakening gates. Record results, remove plan in cleanup commit, push cleanup,
  monitor final run and return PR/check status.

## Evidence

Task 1: branch clean at b2f5e42, tracking origin/feat/openrouter-provider. Inspected
failed logs and current source. Both failures match diagnosis above. No changes
to credentials or application provider behavior are needed.

Task 2: added --features cli to the pinned cargo-about install and contributor
instructions. OAuth gate checks nearest existing directory ancestor before
classifying NotFound as absent; original Windows regression retained, plus nested
missing-path/non-directory ancestor and path-redaction checks. Changelog updated;
permanent verification now records successful initial CI deployment smoke tests.

Passed formatting, all-target/all-feature Clippy -D warnings, full credential-free
test suite (77 deterministic tests, three explicit live skips), warnings-denied
rustdoc, six catalog checks, Ruff, 12 Python tests and actionlint 1.7.12. Installed
cargo-about 0.9.2 --features cli --locked into a fresh temporary root, verified its
executable and successfully ran the notices check using that executable. Fresh
target packaging independently compiled successfully. Next: push and inspect CI;
native Windows execution is verified by its CI job, not this macOS host.
