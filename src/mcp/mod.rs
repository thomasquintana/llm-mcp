//! Stable MCP descriptors and tool dispatch.

use crate::{
    VERSION,
    provider::{Operation, Provider},
};
use rmcp::{
    ErrorData, ServerHandler,
    model::*,
    service::{RequestContext, RoleServer},
};
use serde_json::{Value, json};
use std::{borrow::Cow, sync::Arc};
use tokio_util::sync::CancellationToken;

/// Application handler shared by all transport sessions.
#[derive(Clone)]
pub struct McpServer {
    provider: Provider,
}

impl McpServer {
    /// Wrap an initialized provider.
    pub fn new(provider: Provider) -> Self {
        Self { provider }
    }

    /// Dispatch a tool independently of its transport.
    pub async fn invoke(
        &self,
        name: &str,
        arguments: Value,
        cancel: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let operation = match name {
            "generate" => Operation::Generate,
            "classify" => Operation::Classify,
            _ => return Err(ErrorData::invalid_params("unknown tool", None)),
        };
        let mut result = match self.provider.call(operation, arguments, cancel).await {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => CallToolResult::structured_error(json!({"error": error})),
        };
        // The complete marker is a newer protocol field; preserve the 2025 wire shape.
        result.result_type = None;
        Ok(result)
    }
}

impl ServerHandler for McpServer {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![ProtocolVersion::V_2025_11_25])
    }

    fn get_info(&self) -> ServerConfig {
        server_info()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tools()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools().into_iter().find(|t| t.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.invoke(
            &request.name,
            Value::Object(request.arguments.unwrap_or_default()),
            context.ct,
        )
        .await
        .map(Into::into)
    }
}

/// Deterministic server identity, used by initialize and exported catalogs.
pub fn server_info() -> ServerConfig {
    ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
        .with_server_info(Implementation::new("llm-mcp", VERSION))
        .with_protocol_version(ProtocolVersion::V_2025_11_25)
}

/// Deterministic, forward-compatible descriptors. Options are provider request fields.
pub fn tools() -> Vec<Tool> {
    let schema = json!({
        "type": "object",
        "properties": {
            "model": {"type":"string", "description":"Provider model identifier."},
            "input": {"description":"Full provider input, including conversation and tool history."},
            "stream": {"type":"boolean", "const":false},
            "background": {"type":"boolean", "const":false}
        },
        "additionalProperties": true
    });
    let output = Arc::new(
        json!({"type":"object", "additionalProperties":true})
            .as_object()
            .expect("static object")
            .clone(),
    );
    [
        ("generate", "Call OpenAI Responses with caller-supplied request options. Return the complete response; execute returned function calls in the caller."),
        ("classify", "Call OpenAI Decisions with shared input and named predicate, choice or score questions. Return full answers and probabilities for caller-owned decisions."),
    ].into_iter().map(|(name, description)| {
        let mut input = schema.as_object().expect("static object").clone();
        if name == "classify" {
            input["properties"].as_object_mut().expect("static properties").insert("questions".into(), json!({"type":"array", "minItems":1, "items":{"type":"object"}, "description":"Named predicate, choice and score questions."}));
        }
        Tool::new(name, description, input).with_raw_output_schema(output.clone())
            .with_annotations(ToolAnnotations::new().read_only(false).destructive(false).idempotent(false).open_world(true))
    }).collect()
}

/// Export exact descriptors and identity without requiring provider credentials.
pub fn catalog() -> Value {
    json!({"protocolVersion":crate::MCP_PROTOCOL_VERSION, "serverInfo":server_info().server_info, "tools":tools()})
}
