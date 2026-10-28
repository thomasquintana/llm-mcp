# Changelog

All notable changes follow Semantic Versioning.

## Unreleased

- Add `embed` for OpenAI text/token embeddings and ordered image-description batches, embedding original context plus description with indexed preparation and usage metadata.

- Validate base64-only image inputs for generation and classification, including question instructions and tool-output evidence; reject URL/file references and malformed media before dispatch.

- Select mutually exclusive server-wide authentication with `LLM_MCP_AUTH`; OAuth exposes only generation, and catalogs reflect the selected mode.

- Accept valid OpenAI OAuth event streams with an omitted Content-Type header while retaining terminal-event and size validation.
- Assemble complete OAuth outputs from indexed completed-item events when the terminal response omits them, preserving text and function-call fields.

- Establish the Rust package and stateless MCP server specification.
- Add environment configuration, bounded OpenAI requests and stable generation/classification MCP contracts.
- Add OpenAI browser OAuth, protected credential import/rotation and internal SSE completion handling.
- Serve stdio and Streamable HTTP with cancellation/health endpoints and verified HTLK catalogs/workflows.
- Add a standalone nonroot container, Compose examples and Kubernetes API-key/OAuth deployments.
- Add cross-platform CI, dependency/license gates, coverage and automated crate/binary/container release workflows.
- Use AWS-LC for JWT validation and require issuer, audience, subject and expiry claims.
- Harden OAuth rotation recovery, shutdown, Windows credential ACLs, CPU deadlines and SSE parsing with regression coverage.
- Freeze release artifact receipts and recover interrupted image aliases without replacing published versions.
