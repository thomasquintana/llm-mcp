# llm-mcp

A Rust MCP server exposing OpenAI Responses as `generate` and OpenAI Decisions as
`classify`. Designed for HTLK, standalone containers, and Kubernetes.

Configuration is environment-only. Calls carry provider request options and full
conversation context; results preserve complete provider JSON. Streaming and
background execution are rejected at the MCP boundary. OAuth generation collects
the provider's required stream internally.

See [the specification](docs/specification.md) and [contribution guide](CONTRIBUTING.md).

Licensed under Apache-2.0.
