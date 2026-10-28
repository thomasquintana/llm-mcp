//! Automatic credential-gated E2E tests against the real OpenAI service.
//!
//! A custom runner makes runtime credential skips visible in ordinary cargo test
//! output instead of representing an unexecuted live check as a passing unit test.

#[path = "support/live.rs"]
mod live;
#[path = "support/provider_cases.rs"]
mod provider_cases;

use live::{Credential, Failure, Result, Server, Transport};
use provider_cases::{API_SUITE, OAUTH_SUITE, Selection};
use std::{collections::HashMap, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("FAIL live OpenAI E2E configuration: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<bool> {
    let selection = Selection::parse(std::env::args().skip(1))?;
    if selection.list {
        for name in [API_SUITE, OAUTH_SUITE] {
            if selection.includes(name) {
                println!("{name}: test");
            }
        }
        return Ok(true);
    }
    let mut environment = HashMap::new();
    for key in [
        "OPENAI_API_KEY",
        "HOME",
        "USERPROFILE",
        "LLM_MCP_OAUTH_CREDENTIALS_PATH",
        "LLM_MCP_E2E_TIMEOUT_MS",
        "LLM_MCP_E2E_GENERATE_MODEL",
        "LLM_MCP_E2E_OAUTH_MODEL",
        "LLM_MCP_E2E_CLASSIFY_MODEL",
        "LLM_MCP_E2E_EMBED_MODEL",
        "LLM_MCP_E2E_DESCRIPTION_MODEL",
        "LLM_MCP_E2E_TRANSCRIBE_MODEL",
    ] {
        if let Some(value) = std::env::var_os(key) {
            environment.insert(
                key,
                value
                    .into_string()
                    .map_err(|_| Failure(format!("{key} is not valid Unicode")))?,
            );
        }
    }
    let get = |key: &str| environment.get(key).cloned();
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut filtered = 0;
    let mut checks = 0;
    for name in [API_SUITE, OAUTH_SUITE] {
        if !selection.includes(name) {
            filtered += 1;
            continue;
        }
        let credential = if name == API_SUITE {
            Ok(live::api_key(get))
        } else {
            live::oauth(get)
        };
        let credential = match credential {
            Ok(Some(credential)) => credential,
            Ok(None) => {
                let reason = if name == API_SUITE {
                    "OPENAI_API_KEY is absent/empty"
                } else {
                    "OAuth credential record is absent"
                };
                eprintln!("SKIP {name}: {reason}");
                skipped += 1;
                continue;
            }
            Err(error) => {
                eprintln!("FAIL {name}: {error}");
                failed += 1;
                continue;
            }
        };
        match run_suite(&credential, &get, &mut checks).await {
            Ok(()) => {
                eprintln!("PASS {name}");
                passed += 1;
            }
            Err(error) => {
                eprintln!("FAIL {name}: {error}");
                failed += 1;
            }
        }
    }
    eprintln!(
        "Live OpenAI E2E: {passed} suites passed, {failed} failed, {skipped} skipped, {filtered} filtered out; {checks} checks passed."
    );
    if passed == 0 && failed == 0 {
        eprintln!("No live OpenAI tests executed.");
    }
    Ok(failed == 0)
}

async fn run_suite(
    credential: &Credential,
    get: &impl Fn(&str) -> Option<String>,
    checks: &mut usize,
) -> Result<()> {
    let timeout = provider_cases::timeout(get)?;
    let cases = provider_cases::cases(credential, get)?;
    for transport in [Transport::Stdio, Transport::Http] {
        let context = format!("{}/{}", credential.mode(), transport.name());
        eprintln!("RUN {context}: start real server -> {}", live::OPENAI);
        let mut server = Server::start(
            env!("CARGO_BIN_EXE_llm-mcp"),
            credential,
            transport,
            live::OPENAI,
            timeout,
        )
        .await?;
        if server.pid().is_none() {
            return Err(Failure::new("server process is not running"));
        }
        let outcome = async {
            let listed = server.rpc("tools/list", None).await?;
            provider_cases::check_discovery(&listed, credential)?;
            eprintln!("PASS {context}/discovery");
            *checks += 1;
            for case in &cases {
                eprintln!("RUN {context}/{} model={}", case.name, case.model);
                let result = server.call(case.tool, case.arguments.clone()).await?;
                case.evaluate(&result)
                    .map_err(|error| Failure(format!("{context}/{}: {error}", case.name)))?;
                eprintln!("PASS {context}/{}", case.name);
                *checks += 1;
            }
            Ok(())
        }
        .await;
        // Always finish/reap the process, including after a failed tool assertion.
        let cleanup = server.shutdown().await;
        if let Err(error) = &cleanup {
            eprintln!("FAIL {context}/cleanup: {error}");
        }
        outcome.and(cleanup)?;
    }
    Ok(())
}
