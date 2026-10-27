# Deployment

The same nonroot OCI image serves standalone Docker/Compose and Kubernetes. It
defaults to HTTP on `0.0.0.0:8080`; `/mcp` is the MCP endpoint. Native binaries
default to stdio. Incoming MCP authentication is deferred; deploy HTTP within the
intended private network. Set `LLM_MCP_ALLOWED_HOSTS` for the actual service/ingress
authorities and namespace. Probes report process health, not OpenAI availability,
and never consume model tokens.

## Standalone container

```sh
docker build -t llm-mcp:local .
docker run --rm -p 127.0.0.1:8080:8080 -e OPENAI_API_KEY llm-mcp:local
# Stdio: keep stdin open; do not allocate a TTY.
docker run --rm -i -e OPENAI_API_KEY -e LLM_MCP_TRANSPORT=stdio llm-mcp:local
# Or use the published image:
docker compose -f deploy/compose/api-key.yaml up -d
```

Compose examples use environment injection and versioned images. Replace the
image with `llm-mcp:local` for local builds. Set deadlines, concurrency and byte
limits through environment variables; stop grace must exceed the request deadline
plus ten seconds. SIGTERM marks readiness false and bounds HTTP draining. MCP HTTP
transport may use SSE framing for a single completed result; token output is not
streamed to callers.

## API-key Kubernetes deployment

Create Secret `llm-mcp-openai` with key `api-key` using your secret manager, then:

```sh
kubectl apply -f deploy/kubernetes/api-key.yaml
```

The Deployment starts two replicas. Service ClientIP affinity keeps legacy MCP
protocol sessions on one instance; ingress/load-balancer deployments must preserve
equivalent affinity. Each replica has its own concurrency budget. Rolling updates
or pod loss can end sessions: reconnect and initialize, and let HTLK decide whether
a failed call is safe to retry. Runtime never retries provider calls automatically.

## OAuth Kubernetes deployment

1. Run `llm-mcp login` locally (see [OAuth](oauth.md)).
2. Provision the `llm-mcp-oauth` PVC from the OAuth manifest and keep the Deployment
   scaled to zero during credential import.
3. Create transfer Secret `llm-mcp-oauth-transfer` with key `auth.json` from the
   protected local credential file.
4. Run `deploy/kubernetes/oauth-import.yaml` and wait for successful completion.
5. Delete the transfer Secret, then start the OAuth Deployment at one replica.

The import job preserves/creates destination host identity and writes rotating
state to `/state`. The OAuth Deployment uses `Recreate`, one replica, and a writable
PVC. Do not scale it or reuse its credential record in another process. Ensure the
PVC permits UID/GID 65532 to write. Both pods use `fsGroupChangePolicy:
OnRootMismatch`; an owner-run init container restores file mode 0600 after mounting
because some storage drivers add group permissions on remount. Credential import is explicit; it is not rerun
on every pod restart. Re-import with the server stopped when reauthorization is
needed. OAuth enables only `generate`; do not supply a platform API key to this deployment.

For Docker OAuth, use the named volume in `deploy/compose/oauth.yaml`, import into
it with the server stopped, then start Compose. Read-only root filesystems work
because only the explicitly mounted `/state` must be writable.

For example, after local login, securely feed the credential record over stdin so
the nonroot container can create its own owner-only transfer file:

```sh
docker volume create llm-mcp-oauth
docker run --rm -i --read-only -v llm-mcp-oauth:/state \
  --entrypoint /bin/sh ghcr.io/thomasquintana/llm-mcp:0.1.0 -ec \
  'umask 077; trap "rm -f /state/import.json" EXIT; cat > /state/import.json; llm-mcp auth import /state/import.json' \
  < "$HOME/.config/llm-mcp/auth.json"
docker compose -f deploy/compose/oauth.yaml up -d
```

The Compose OAuth example names that volume explicitly. Give separate credential
records/volumes to separate instances.

Release Linux binaries use a Debian bookworm build baseline (glibc 2.36); container
runtime uses the same baseline. ARM64 and x86-64 images are built natively in release
CI. Pin the resulting image digest in production manifests.
