# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
FROM rust:1.99-bookworm@sha256:48b7f435b8fc6378ce43b44f15e3a47fd8919b2825f835c39bd4bd3da50af5b8 AS build
ENV RUSTUP_TOOLCHAIN=1.98.0
RUN apt-get update && apt-get install -y --no-install-recommends cmake clang pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
# Cargo scans explicit test targets even for a release-only build.
COPY tests ./tests
COPY examples ./examples
COPY README.md LICENSE THIRD_PARTY_LICENSES ./
RUN cargo build --release --locked

FROM scratch AS export
COPY --from=build /src/target/release/llm-mcp /llm-mcp
COPY LICENSE THIRD_PARTY_LICENSES /

FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8 AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libgcc-s1 && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /state && chown 65532:65532 /state
COPY --from=build /src/target/release/llm-mcp /usr/local/bin/llm-mcp
COPY LICENSE THIRD_PARTY_LICENSES /usr/share/licenses/llm-mcp/
LABEL org.opencontainers.image.title="llm-mcp" \
      org.opencontainers.image.source="https://github.com/thomasquintana/llm-mcp" \
      org.opencontainers.image.licenses="Apache-2.0"
ENV LLM_MCP_TRANSPORT=http LLM_MCP_BIND=0.0.0.0:8080 LLM_MCP_OAUTH_CREDENTIALS_PATH=/state/auth.json
USER 65532:65532
EXPOSE 8080
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/llm-mcp"]
CMD ["serve"]
