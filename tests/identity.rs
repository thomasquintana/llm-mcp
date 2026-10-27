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
