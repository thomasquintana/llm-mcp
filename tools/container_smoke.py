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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", default="llm-mcp:local")
    args = parser.parse_args()
    mock = http.server.ThreadingHTTPServer(("0.0.0.0", 0), Provider)
    threading.Thread(target=mock.serve_forever, daemon=True).start()
    container = None
    volume = f"llm-mcp-test-{uuid.uuid4().hex}"
    try:
        container = docker(
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
            f"OPENAI_BASE_URL=http://host.docker.internal:{mock.server_port}/v1/",
            args.image,
        )
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
                        "arguments": {"model": "mock", "input": "hello"},
                    },
                },
                session,
            )
            assert not response["result"]["isError"], response
            assert isinstance(response["result"]["structuredContent"], dict)
        docker("stop", "--time=10", container)
        assert docker("inspect", "--format={{.State.ExitCode}}", container) == "0"
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
        print(
            "Container HTTP generation/classification, graceful termination and persistent nonroot OAuth state passed."
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
