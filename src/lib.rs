//! Stateless OpenAI generation, classification, embedding and transcription over MCP.

pub mod auth;
pub mod config;
pub mod error;
pub mod mcp;
pub mod provider;
pub mod runtime;

/// The package version used in MCP server identity and catalogs.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP protocol version required by the initial HTLK integration.
pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
