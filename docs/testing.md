# End-to-end and live-provider testing

`cargo test --all-features --locked` includes **automatic, credential-gated tests
against real OpenAI and OpenRouter services**. There is no extra opt-in flag or `#[ignore]` gate.
These run alongside deterministic unit, mock-provider and MCP transport tests.

## Independent credential gates

| Suite | Gate | Live coverage over both stdio and HTTP |
|---|---|---|
| `live_openai_api_key` | Nonempty `OPENAI_API_KEY` | Generation text/images, classification text/images, text embeddings, multi-image contextual embeddings and spoken-audio transcription |
| `live_openai_oauth` | Existing OAuth credential record or pending recovery state | Generation text/images, generation-only discovery and rejection of classification/embedding/transcription |
| `live_openrouter_api_key` | Nonempty `OPENROUTER_API_KEY` | Generation text/images and function continuation, native alpha Decisions text/images, text/contextual-image embeddings and spoken-audio transcription |

OAuth uses `LLM_MCP_OAUTH_CREDENTIALS_PATH` or the server's normal default
`$HOME/.config/llm-mcp/auth.json` (`USERPROFILE` on Windows). Tests use the existing
store and refresh machinery; they never perform browser login or copy rotating
credentials into a temporary store. The OAuth record must have one owner: stop any
server using that record before testing. Normal refresh may update the original
credential state.

All credentials may be available to the test runner. Each child server still uses
one provider and authentication mode. API-key children receive only their selected
provider's API key; OAuth children have both API keys removed. Providers, modes and transports run
sequentially, releasing the OAuth record before starting the next process.

### Skip versus failure

- Missing/empty API key: skip the API-key suite.
- Missing OAuth credential and pending files: skip the OAuth suite.
- Existing but invalid/unreadable/locked credentials: fail, not skip.
- Refresh, provider authentication, unavailable model, rate limit, timeout or result
  assertion failures: fail, not skip. Provider requests are not automatically retried.

`provider_e2e` is a custom Cargo test runner so ordinary test output visibly shows
`RUN`, `PASS`, `FAIL`, `SKIP`, and a summary. An unexecuted provider suite is not
silently counted as a passing unit test. With no credentials, output includes:

```text
SKIP live_openai_api_key: OPENAI_API_KEY is absent/empty
SKIP live_openai_oauth: OAuth credential record is absent
SKIP live_openrouter_api_key: OPENROUTER_API_KEY is absent/empty
Live provider E2E: 0 suites passed, 0 failed, 3 skipped, 0 filtered out; 0 checks passed.
No live provider tests executed.
```

Any failed suite makes the target exit unsuccessfully. A failed suite aborts its
remaining cases, cleans up its server, and still allows the other selected
authentication suite to be attempted.

## Run commands

```sh
# All normal and credential-gated live tests:
cargo test --all-features --locked

# Only live-provider suites (still credential-gated):
cargo test --all-features --locked --test provider_e2e

# Select one authentication suite:
cargo test --locked --test provider_e2e live_openai_api_key
cargo test --locked --test provider_e2e live_openai_oauth
cargo test --locked --test provider_e2e live_openrouter_api_key

# List suites without accessing credentials or making provider requests:
cargo test --locked --test provider_e2e -- --list

# Explicitly filter live suites out of a deterministic-only run:
cargo test --all-features --locked -- --skip live_openai_ --skip live_openrouter_
```

Standard name filters, `--exact`, `--skip`, `--list`, `--nocapture`, and
`--test-threads` are accepted. There are no ignored live cases, so `--ignored`
selects none. Selecting an unrelated unit-test name also filters out all live
suites. Without filters, credential availability alone activates each suite.

Live requests incur normal provider usage. Repeated test or coverage runs repeat the
live cases whenever credentials are present. A credential-free environment can run
all deterministic checks, including regressions for the live-test driver and its
assertions, without contacting providers. Tests do not load `.env` automatically;
export its settings in your test shell when you intend to run live checks.

## Models and deadlines

Tests choose their own isolated application settings rather than inheriting
`LLM_MCP_PROVIDER`, `LLM_MCP_AUTH`, a development mock URL, or unrelated server limits.
Live provider URLs are pinned to **`https://api.openai.com/v1/`** and
**`https://openrouter.ai/api/v1/`**; base URL environment overrides cannot turn
these suites into mock-provider tests. OpenAI API-key organization/project settings
and OS TLS/proxy configuration are retained. The local MCP HTTP client bypasses
proxies and rejects redirects.

| Test setting | Default | Purpose |
|---|---|---|
| `LLM_MCP_E2E_GENERATE_MODEL` | `gpt-4.1-mini` | API-key text/image generation |
| `LLM_MCP_E2E_OAUTH_MODEL` | `gpt-6.1-sol` | OAuth text/image generation; choose an account-available model |
| `LLM_MCP_E2E_CLASSIFY_MODEL` | `gpt-6-luna` | Decisions predicate/choice questions |
| `LLM_MCP_E2E_EMBED_MODEL` | `text-embedding-3-small` | Float embeddings with 64 dimensions |
| `LLM_MCP_E2E_DESCRIPTION_MODEL` | `gpt-4.1-mini` | Detailed multi-image description within `embed` |
| `LLM_MCP_E2E_TRANSCRIBE_MODEL` | `gpt-4o-mini-transcribe` | JSON transcription of the spoken fixture |
| `LLM_MCP_E2E_OPENROUTER_GENERATE_MODEL` | `openai/gpt-4.1-mini` | OpenRouter text/image generation and function continuation |
| `LLM_MCP_E2E_OPENROUTER_CLASSIFY_MODEL` | `typesafe/jev-1.14` | Native noul/choice classification with text and a 512×512 PNG |
| `LLM_MCP_E2E_OPENROUTER_EMBED_MODEL` | `openai/text-embedding-3-small` | OpenRouter float embeddings with 64 dimensions |
| `LLM_MCP_E2E_OPENROUTER_DESCRIPTION_MODEL` | `openai/gpt-4.1-mini` | OpenRouter multi-image descriptions within embed |
| `LLM_MCP_E2E_OPENROUTER_TRANSCRIBE_MODEL` | `openai/whisper-large-v3` | OpenRouter JSON audio transcription |
| `LLM_MCP_E2E_TIMEOUT_MS` | `120000` | Per-tool server deadline |

Overrides must support the scenario's modalities/options; an unavailable or
incompatible model fails rather than skips. Embedding overrides must accept
`dimensions:64` and `encoding_format:"float"`. HTTP client/driver calls have a small
additional allowance for MCP framing and cleanup. Process startup, response bytes,
and shutdown are also bounded.

Example:

```sh
LLM_MCP_E2E_OAUTH_MODEL=gpt-6.1-sol cargo test --locked --test provider_e2e live_openai_oauth
```

## What is verified

Each transport launches the actual compiled `llm-mcp` binary. The independent
driver sends MCP initialize, notification, discovery and tool requests, verifies
structured results match their JSON text representation, and reaps the process on
success or failure. It does not use RMCP's client implementation or a mock provider
for live runs. Logs include mode, transport, case, model, sanitized error category,
HTTP status and request ID when available; they exclude credentials, raw media and
credential-store contents.

The image fixture contains a red square on the left, a blue circle on the right,
and labels `RED 42` and `BLUE`. The speech says “The red square is next to the blue
circle.” Small attributable synthetic assets and their generator/provenance are
under [`tests/fixtures`](../tests/fixtures/README.md).

Assertions check recognizable words/colors/shapes/printed numbers, confident
classification answers, nonzero finite vectors of the expected dimensions/count,
input-index mapping, original context plus a usable image description, returned
description model/usage, and expected transcript words. Multiple related images
produce one vector for their item, followed by an independent text-only item. The
tests avoid exact generated prose, exact vectors and transcript punctuation.
OpenRouter adds a real two-call function continuation: a forced function call is
returned to the caller, which supplies a synthetic result in full history; the
follow-up must identify that result. Classification assertions inspect native
named answers and probabilities rather than OpenAI's answer array.

## Other integration layers

- `tests/live_harness.rs`: offline driver/gate/assertion regressions, including
  invalid credentials, authentication isolation, failure redaction and both real
  server transports against a local provider.
- `tests/provider.rs`, `tests/openrouter.rs`, `tests/configuration.rs`, `tests/embed.rs`, `tests/transcribe.rs`: deterministic
  provider contracts, failures, multipart bytes, cancellation and resource bounds.
- `tests/transports.rs`: independent MCP wire tests with mock providers.
- `python3 tools/check_htlk.py`: real sibling HTLK compiler/runtime workflows with
  a mock provider over both transports.
- `python3 tools/container_smoke.py --image <built-image>`: nonroot container,
  authentication modes, credential persistence and shutdown with a mock provider.

CI without provider credentials reports the live suites as skipped and continues
to exercise these offline layers. Environments with credentials automatically run
the corresponding live suites. Actual execution evidence and remaining external
boundaries are recorded in [verification](verification.md).
