#!/usr/bin/env python3
"""Exercise the standalone image against a host-local provider and writable state."""

import argparse
import http.server
import json
import subprocess
import threading
import time
import urllib.request
import uuid
from pathlib import Path

from check_htlk import Provider


def docker(*args):
    return subprocess.check_output(["docker", *args], text=True).strip()


def rpc(url, body, session=None):
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
        "MCP-Protocol-Version": "2025-11-25",
    }
    if session:
        headers["Mcp-Session-Id"] = session
    with urllib.request.urlopen(
        urllib.request.Request(url, json.dumps(body).encode(), headers), timeout=15
    ) as response:
        session = response.headers.get("Mcp-Session-Id", session)
        text = response.read().decode()
    if not text:
        return {}, session
    try:
        return json.loads(text), session
    except json.JSONDecodeError:
        return next(
            json.loads(line[5:].strip())
            for line in text.splitlines()
            if line.startswith("data:") and line[5:].strip()
        ), session


def exercise(container):
    port = docker("port", container, "8080/tcp").rsplit(":", 1)[1]
    base = f"http://127.0.0.1:{port}"
    for attempt in range(100):
        try:
            urllib.request.urlopen(f"{base}/health/ready", timeout=1).close()
            break
        except OSError:
            if attempt == 99:
                raise
            time.sleep(0.05)
    url = f"{base}/mcp"
    response, session = rpc(
        url,
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "container-smoke", "version": "1"},
            },
        },
    )
    assert response["result"]["serverInfo"]["name"] == "llm-mcp"
    rpc(url, {"jsonrpc": "2.0", "method": "notifications/initialized"}, session)
    for index, tool in enumerate(("generate", "classify"), 2):
        response, _ = rpc(
            url,
            {
                "jsonrpc": "2.0",
                "id": index,
                "method": "tools/call",
                "params": {
                    "name": tool,
                    "arguments": {
                        "model": "mock",
                        "input": [{"role": "user", "content": "hello"}],
                    },
                },
            },
            session,
        )
        assert not response["result"]["isError"], response
        assert isinstance(response["result"]["structuredContent"], dict)
        value = response["result"]["structuredContent"]
        if tool == "generate":
            text = "".join(
                part.get("text", "")
                for item in value["output"]
                for part in item.get("content", [])
                if part.get("type") == "output_text"
            )
            assert text == "Hello", "successful generation must retain its output text"
        else:
            assert value["answers"][0]["probability"] == 0.95


def start(image, port, extra=()):
    return docker(
        "run",
        "-d",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--add-host=host.docker.internal:host-gateway",
        "-p",
        "127.0.0.1::8080",
        "-e",
        "OPENAI_API_KEY=integration-test",
        "-e",
        f"OPENAI_BASE_URL=http://host.docker.internal:{port}/v1/",
        *extra,
        image,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", default="llm-mcp:local")
    args = parser.parse_args()
    mock = http.server.ThreadingHTTPServer(("0.0.0.0", 0), Provider)
    threading.Thread(target=mock.serve_forever, daemon=True).start()
    container = None
    volume = f"llm-mcp-test-{uuid.uuid4().hex}"
    try:
        container = start(args.image, mock.server_port)
        exercise(container)
        docker("stop", "--time=10", container)
        assert docker("inspect", "--format={{.State.ExitCode}}", container) == "0"
        docker("rm", container)
        container = None
        docker("volume", "create", volume)
        status = json.loads(
            docker(
                "run",
                "--rm",
                "--read-only",
                "--mount",
                f"type=volume,source={volume},target=/state",
                args.image,
                "auth",
                "status",
            )
        )
        assert not status["signed_in"]
        again = json.loads(
            docker(
                "run",
                "--rm",
                "--read-only",
                "--mount",
                f"type=volume,source={volume},target=/state",
                args.image,
                "auth",
                "status",
            )
        )
        assert status["host_id"] == again["host_id"]
        mount = f"type=volume,source={volume},target=/state"
        credentials = {
            "issuer": "https://auth.openai.com",
            "subject": "mock-user",
            "email": None,
            "client_id": "oaiapp_mock",
            "ext_agent_host_id": f"urn:uuid:{uuid.uuid4()}",
            "id_token": "mock-retained-id",
            "access_token": "oauth-integration-test",
            "refresh_token": "mock-refresh",
            "scopes": ["chatgpt.tokens.use.direct"],
            "expires_at": int(time.time()) + 3600,
        }
        subprocess.run(
            [
                "docker",
                "run",
                "--rm",
                "-i",
                "--read-only",
                "--mount",
                mount,
                "--entrypoint",
                "/bin/sh",
                args.image,
                "-ec",
                "umask 077; trap 'rm -f /state/import.json' EXIT; cat > /state/import.json; llm-mcp auth import /state/import.json",
            ],
            input=json.dumps(credentials),
            text=True,
            check=True,
        )
        docker(
            "run",
            "--rm",
            "--read-only",
            "--mount",
            mount,
            "--entrypoint",
            "/bin/sh",
            args.image,
            "-ec",
            "chmod 660 /state/auth.json",
        )
        rejected = subprocess.run(
            [
                "docker",
                "run",
                "--rm",
                "--read-only",
                "--mount",
                mount,
                args.image,
                "auth",
                "status",
            ],
            capture_output=True,
        )
        assert rejected.returncode != 0, "group-readable credentials must be rejected"
        manifest = (
            Path(__file__).resolve().parents[1] / "deploy/kubernetes/oauth.yaml"
        ).read_text()
        command = next(
            json.loads(line.strip().removeprefix("args: "))[0]
            for line in manifest.splitlines()
            if line.strip().startswith('args: ["find /state')
        )
        docker(
            "run",
            "--rm",
            "--read-only",
            "--mount",
            mount,
            "--entrypoint",
            "/bin/sh",
            args.image,
            "-ec",
            command,
        )
        imported = json.loads(
            docker(
                "run",
                "--rm",
                "--read-only",
                "--mount",
                mount,
                args.image,
                "auth",
                "status",
            )
        )
        assert imported["signed_in"] and imported["host_id"] == status["host_id"]
        container = start(
            args.image,
            mock.server_port,
            ("--mount", mount, "-e", "LLM_MCP_GENERATE_AUTH=oauth"),
        )
        exercise(container)
        docker("stop", "--time=10", container)
        assert docker("inspect", "--format={{.State.ExitCode}}", container) == "0"
        print(
            "Container API-key/OAuth generation, classification, graceful termination, credential import and remount permission repair passed."
        )
    finally:
        if container:
            docker("rm", "-f", container)
        subprocess.run(
            ["docker", "volume", "rm", volume], check=False, capture_output=True
        )
        mock.shutdown()
        mock.server_close()


if __name__ == "__main__":
    main()
