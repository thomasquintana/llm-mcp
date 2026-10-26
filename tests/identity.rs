//! Server identity must match its published package and HTLK catalog.

#[test]
fn identity_matches_package() {
    assert_eq!(llm_mcp::VERSION, env!("CARGO_PKG_VERSION"));
    assert_eq!(llm_mcp::MCP_PROTOCOL_VERSION, "2025-11-25");
}
