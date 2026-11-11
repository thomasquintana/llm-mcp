# Complete user guide implementation plan

## Objective and working rules

Create `docs/user-guide.md`: a complete, human-readable, task-oriented manual for
llm-mcp, grounded in implementation, tests, deployment manifests and existing docs.
Link it prominently from README and correct directly related documentation drift.

- Branch: `docs/complete-user-guide`, based on `origin/main` (`4f2bb08`).
- Reread this plan before work, after each task, and immediately after compaction.
- Update this plan with outcomes and verification after every task completes.
- Commit each completed task, including its plan update; inspect status, diff and
  recent history before committing. Stage only intended files.
- When finished, ensure all changes are committed, push and open a GitHub PR.
- This plan is temporary working material in the user-requested location. Keep it
  through review so progress and decisions are recorded with each task commit.

## Analysis already completed

Reviewed CLI/config, MCP contracts, provider/media adapters, OAuth store and refresh,
error handling, runtime transports, integration tests, HTLK examples, Docker and
Compose/Kubernetes manifests, contributing/testing/release/verification documents.
The current branch's source tree matches the analyzed tree.

Key facts: native stdio/container HTTP defaults; MCP 2025-11-25; one provider and auth
mode per server; API keys enable four tools, OpenAI OAuth only generate; no dotenv
loading; distinct OpenRouter Decisions format; base64-only image/audio contracts;
image embeddings use descriptions; caller owns function execution/history; completed
results, queue-inclusive deadlines, no automatic provider retries; HTTP protocol
sessions require affinity; incoming MCP authentication is not implemented.

Known documentation corrections: release docs' outdated no-live-test statement;
OAuth Kubernetes bootstrap must explicitly provision zero replicas before import.

## Tasks and acceptance criteria

### Task 1 — branch and saved plan [completed]

Created documentation branch from fetched main, confirmed clean working tree and
GitHub authentication, saved this plan. No configuration files or credentials read.
Verification: `git diff --stat HEAD origin/main` was empty before branching.

### Task 2 — complete guide [completed]

Created `docs/user-guide.md` with all 16 navigable sections, three credential paths,
complete curl MCP session, four tool tutorials, native provider differences, full
configuration/CLI references, OAuth lifecycle, Docker/Compose/Kubernetes bootstrap,
HTLK, troubleshooting, deterministic/live testing and upgrade procedures. Cross-
checked the draft against the previously reviewed source and fixtures. Preliminary
whitespace review passed; executable example/link checks remain Task 4.

Write one navigable guide with these sections:
1. Introduction, glossary concepts, architecture and capability matrix.
2. Installation: source/registry/binaries/container, Rust 1.98+, targets/checksums.
3. Quick starts: OpenAI API key, OpenRouter API key, ChatGPT OAuth.
4. MCP clients: stdio settings, HTTP session example, initialization/discovery,
   exact protocol headers, tool arguments versus JSON-RPC envelopes.
5. Generate: text/images, structured output, reasoning, full-history continuation.
6. Classify: separate OpenAI and native OpenRouter questions/results/thresholds.
7. Embed: text/tokens, contextual image pipeline, ordering/prepared text/usage/errors.
8. Transcribe: client-side encoding, MIME/extensions, options and result formats.
9. Results/errors: structuredContent/text, error fields/codes, retries/cancellation.
10. Full environment configuration reference and resource tuning.
11. OAuth lifecycle: login/status/import/logout, permissions/locks/rotation/recovery.
12. Docker/Compose/Kubernetes: deployment, OAuth bootstrap, affinity/scaling/shutdown.
13. HTLK: matching catalogs, workflow examples and upgrade regeneration.
14. Operations/troubleshooting/testing/upgrades, CLI reference and glossary/links.

Use numbered procedures, complete JSON, expected outcomes, clear model/provider
requirements, POSIX/PowerShell distinctions, and runnable versus illustrative labels.
Use implementation as source of truth. Do not imply publication/model availability
or historical verification proves present access. No application behavior changes.

### Task 3 — documentation integration [completed]

Added prominent README guide link and Unreleased entry. Updated release docs to
describe automatic credential-gated live tests. Clarified zero-replica OAuth
Kubernetes bootstrap and fresh import Jobs, with a direct guide link. Reviewed the
four-file diff and passed `git diff --check`; no application/manifests changed.

Add README entry point, Unreleased changelog entry, correct release testing text and
OAuth bootstrap instructions. Keep links relative and preserve focused references.

### Task 4 — validation and refinements [completed]

Validation completed on macOS ARM64, Rust/Cargo 1.98.0. Refined archive checksum
commands into copyable shell syntax and clarified empty SSE events/chunk assembly
after executing the guide's exact curl commands. No application changes needed.

Passing checks:
- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `cargo test --all-features --locked -- --skip live_openai_ --skip live_openrouter_`
  (77 deterministic regressions; 3 live suites explicitly filtered out)
- `RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps --locked`
- `CARGO_TARGET_DIR="$(mktemp -d /var/folders/gb/g744mwgj3nn6mtf4dgdg58p00000gn/T/opencode/llm-mcp-user-guide-package.XXXXXX)" cargo package --locked --allow-dirty`
  (fresh independent compilation; 81 packaged files)
- `python3 tools/check_notices.py`
- `python3 tools/export_catalogs.py --check`
- `uv run --with-requirements tools/requirements.txt ruff check tools`
- `uv run --with-requirements tools/requirements.txt python -m unittest discover -s tools -p 'test_*.py'`
  (12 Python regressions)
- `git diff --check`

One-off verification script lives outside the repo at
`/var/folders/gb/g744mwgj3nn6mtf4dgdg58p00000gn/T/opencode/validate-llm-mcp-guide.py`.
Ran with `uv run --with pyyaml --with jsonschema python3 <script>`: 15 JSON examples,
70 local links/anchors, 35 shell blocks, argument examples against exported schemas,
all 17 config/log variables, CLI help/version and six provider/auth/transport HTLK
catalog combinations. Exact Kubernetes bootstrap produced PVC, zero-replica
Deployment and Service. Exact five documented curl commands initialized, notified,
discovered, generated, and deleted a real server session against a local mock;
checked structured/text parity and one upstream call. No real credentials used.

Nonpassing existing gate: `cargo deny check` fails RUSTSEC-2026-0462 against locked
rmcp 3.7.0 (advisory describes built-in Streamable HTTP client response buffering).
Cargo.toml/Cargo.lock match origin/main exactly; dependency updates are outside
this documentation task. Existing duplicate-version/unused-license warnings also
remain. Record the audit failure explicitly in the PR/final report. Notices and
catalogs were run separately after the audit stopped its command chain and passed.
No live cluster, provider or new image build is claimed by these docs checks.

Validate links/anchors and JSON examples; exercise the documented MCP HTTP sequence
against a local mock provider and compare CLI help/catalog behavior. Inspect all
guide claims against code. Run applicable CONTRIBUTING checks, using documented
live-suite skip filters for deterministic verification. Do not source .env or access
real credentials. Record exact checks and any environment limitations. Fix findings
and commit validation/refinements with this plan update.

Required Rust checks: fmt, all-target/all-feature Clippy with denied warnings,
deterministic tests, denied-warning rustdoc, fresh-target cargo package.
Additional relevant checks: catalog drift, notices/dependency policy if available;
docs-only changes do not require repeating image builds/live provider tests.

### Task 5 — final review and pull request [pending]

Review status, full base diff, all branch commits, tracking and recent history;
ensure no credentials or unrelated artifacts. Record completion and verification in
this plan, commit before pushing. Push branch and open PR against main with summary
and checks. Verify clean status and PR URL. If recording the URL afterward changes
the plan, commit and push that update too.

## Progress / execution evidence

- Task 1: branch and plan ready; starting Task 2 after the plan commit.
- Task 2: guide draft complete. Reread the plan for alignment: every planned topic
  is present; next integrate README, changelog and the two focused corrections.
- Task 3: integration complete. Reread plan; next validate runnable examples,
  links/anchors, contracts and required checks before final review/publication.
- Task 4: verification and refinements complete, including recorded baseline
  audit failure. Reread plan: all guide/integration criteria covered; next review
  all commits/base diff, commit this evidence, push, and open the PR.
