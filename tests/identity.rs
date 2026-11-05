//! Server identity must match its published package and HTLK catalog.

#[test]
fn identity_matches_package() {
    assert_eq!(llm_mcp::VERSION, env!("CARGO_PKG_VERSION"));
    assert_eq!(llm_mcp::MCP_PROTOCOL_VERSION, "2025-11-25");
}

#[test]
fn catalogs_are_mode_specific_and_export_without_credentials() {
    use llm_mcp::{config::AuthMode, mcp};
    for (mode, auth) in [("api_key", AuthMode::ApiKey), ("oauth", AuthMode::OAuth)] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_llm-mcp"))
            .args(["catalog"])
            .env_remove("OPENAI_API_KEY")
            .env("LLM_MCP_PROVIDER", "openai")
            .env("LLM_MCP_AUTH", mode)
            .env(
                "LLM_MCP_OAUTH_CREDENTIALS_PATH",
                "/nonexistent/catalog/auth.json",
            )
            .output()
            .unwrap();
        assert!(output.status.success());
        let catalog: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(catalog, mcp::catalog(auth));
        let tools = mcp::tools(auth);
        assert_eq!(catalog["tools"], serde_json::to_value(&tools).unwrap());
        let htlk = mcp::htlk_catalog("stdio", auth);
        assert_eq!(
            htlk["tools"]["tools"].as_array().unwrap().len(),
            tools.len()
        );
        if auth == AuthMode::OAuth {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].name, "generate");
        }
    }
}

#[test]
fn openrouter_catalog_uses_native_schemas_without_credentials() {
    use llm_mcp::{
        config::{AuthMode, ServiceProvider},
        mcp,
    };
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_llm-mcp"))
        .arg("catalog")
        .env("LLM_MCP_PROVIDER", "openrouter")
        .env("LLM_MCP_AUTH", "api_key")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENROUTER_API_KEY")
        .output()
        .unwrap();
    assert!(output.status.success());
    let catalog: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        catalog,
        mcp::catalog_for(ServiceProvider::OpenRouter, AuthMode::ApiKey)
    );
    let tools = catalog["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 4);
    let classify = tools.iter().find(|t| t["name"] == "classify").unwrap();
    assert_eq!(
        classify["inputSchema"]["required"],
        serde_json::json!(["model", "state", "questions"])
    );
    assert_eq!(
        classify["inputSchema"]["properties"]["questions"]["type"],
        "object"
    );
    assert!(classify["inputSchema"]["properties"].get("input").is_none());
    let transcribe = tools.iter().find(|t| t["name"] == "transcribe").unwrap();
    assert_eq!(
        transcribe["inputSchema"]["properties"]["response_format"]["enum"],
        serde_json::json!(["json", "verbose_json"])
    );
    for args in [vec!["login"], vec!["auth", "status"]] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_llm-mcp"))
            .args(args)
            .env("LLM_MCP_PROVIDER", "openrouter")
            .env("LLM_MCP_AUTH", "api_key")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("OpenAI-only"));
    }
}
