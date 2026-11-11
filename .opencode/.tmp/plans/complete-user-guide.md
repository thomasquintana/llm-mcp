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

### Task 2 — complete guide [pending]

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

### Task 3 — documentation integration [pending]

Add README entry point, Unreleased changelog entry, correct release testing text and
OAuth bootstrap instructions. Keep links relative and preserve focused references.

### Task 4 — validation and refinements [pending]

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
