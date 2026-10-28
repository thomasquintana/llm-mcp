#!/usr/bin/env python3
"""Opt-in real sibling HTLK integration over both transports, using a local provider."""

import argparse
import base64
from email import policy
from email.parser import BytesParser
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


class Provider(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        payload = self.rfile.read(int(self.headers["Content-Length"]))
        if self.path.endswith("audio/transcriptions"):
            message = BytesParser(policy=policy.default).parsebytes(
                f"Content-Type: {self.headers['Content-Type']}\r\nMIME-Version: 1.0\r\n\r\n".encode()
                + payload
            )
            parts = {
                part.get_param("name", header="Content-Disposition"): part
                for part in message.iter_parts()
            }
            example = json.loads((ROOT / "examples/htlk/requests.json").read_text())
            audio = example["transcription"]["file"]
            assert parts["file"].get_payload(decode=True) == base64.b64decode(
                audio["data_base64"]
            )
            assert parts["file"].get_filename() == audio["filename"]
            assert parts["file"].get_content_type() == audio["mime_type"]
            request = {"model": parts["model"].get_payload(decode=True).decode()}
        else:
            request = json.loads(payload)
        oauth = self.path.endswith(
            "responses"
        ) and "text/event-stream" in self.headers.get("Accept", "")
        assert self.headers["Authorization"] == (
            "Bearer oauth-integration-test" if oauth else "Bearer integration-test"
        )
        assert "model" in request
        if self.path.endswith("responses"):
            response = {
                "id": "resp_test",
                "status": "completed",
                "output": [
                    {
                        "type": "message",
                        "content": [{"type": "output_text", "text": "Hello"}],
                    }
                ],
            }
        elif self.path.endswith("decisions"):
            response = {
                "answers": [
                    {"name": "refund", "type": "predicate", "probability": 0.95}
                ]
            }
        elif self.path.endswith("embeddings"):
            inputs = request["input"]
            if isinstance(inputs, str):
                inputs = [inputs]
            assert all(isinstance(text, str) for text in inputs)
            response = {
                "object": "list",
                "model": request["model"],
                "data": [
                    {"object": "embedding", "index": index, "embedding": [0.1, 0.2]}
                    for index in range(len(inputs))
                ],
                "usage": {"total_tokens": 9},
            }
        elif self.path.endswith("audio/transcriptions"):
            response = {"text": "Transcription OK.", "usage": {"total_tokens": 7}}
        else:
            raise AssertionError(f"unexpected provider endpoint: {self.path}")
        if oauth:
            # Match live OAuth: no Content-Type and metadata-only terminal output.
            events = [
                {
                    "type": "response.output_item.added",
                    "output_index": 0,
                    "item": {"type": "message", "content": []},
                },
                {
                    "type": "response.output_item.done",
                    "output_index": 0,
                    "item": response["output"][0],
                },
                {"type": "response.completed", "response": {**response, "output": []}},
            ]
            body = "".join(
                f"data: {json.dumps(event)}\n\n" for event in events
            ).encode()
        else:
            body = json.dumps(response).encode()
        self.send_response(200)
        if not oauth:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--htlk", type=Path, default=ROOT.parent / "htlk")
    args = parser.parse_args()
    subprocess.run(["cargo", "build", "--locked"], cwd=ROOT, check=True)
    binary = (ROOT / "target/debug/llm-mcp").resolve()
    mock = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    threading.Thread(target=mock.serve_forever, daemon=True).start()
    settings = {
        "OPENAI_API_KEY": "integration-test",
        "OPENAI_BASE_URL": f"http://127.0.0.1:{mock.server_port}/v1/",
        "LLM_MCP_AUTH": "api_key",
    }
    try:
        for transport in ("stdio", "streamable_http"):
            process = None
            try:
                if transport == "stdio":
                    endpoint = {
                        "transport": "stdio",
                        "program": str(binary),
                        "arguments": ["serve"],
                        "environment": {**settings, "LLM_MCP_TRANSPORT": "stdio"},
                    }
                else:
                    with socket.socket() as sock:
                        sock.bind(("127.0.0.1", 0))
                        port = sock.getsockname()[1]
                    process = subprocess.Popen(
                        [str(binary), "serve"],
                        env={
                            **os.environ,
                            **settings,
                            "LLM_MCP_TRANSPORT": "http",
                            "LLM_MCP_BIND": f"127.0.0.1:{port}",
                        },
                    )
                    for attempt in range(100):
                        try:
                            urllib.request.urlopen(
                                f"http://127.0.0.1:{port}/health/ready", timeout=1
                            ).close()
                            break
                        except OSError:
                            if process.poll() is not None or attempt == 99:
                                raise
                            time.sleep(0.05)
                    endpoint = {
                        "transport": "streamable_http",
                        "url": f"http://127.0.0.1:{port}/mcp",
                        "headers": {},
                    }
                with tempfile.TemporaryDirectory(prefix="llm-mcp-htlk-") as directory:
                    config = {
                        "source": str(ROOT / "examples/htlk/workflow.htlk"),
                        "catalogs": str(
                            ROOT / f"examples/htlk/{transport}.catalogs.json"
                        ),
                        "inputs": json.loads(
                            (ROOT / "examples/htlk/requests.json").read_text()
                        ),
                        "endpoints": {"llm-mcp": endpoint},
                    }
                    path = Path(directory) / "host.json"
                    path.write_text(json.dumps(config))
                    subprocess.run(
                        [
                            "cargo",
                            "run",
                            "--locked",
                            "--example",
                            "mcp_capability_spike",
                            "--",
                            str(path),
                        ],
                        cwd=args.htlk,
                        check=True,
                        timeout=300,
                    )
                    config["source"] = str(ROOT / "examples/htlk/continuation.htlk")
                    config["inputs"] = {
                        "model": "mock-model",
                        "user": {"role": "user", "content": "What is the weather?"},
                        "call": {
                            "type": "function_call",
                            "name": "weather",
                            "arguments": "{}",
                            "call_id": "call_weather",
                        },
                        "function_result": "Sunny",
                    }
                    path.write_text(json.dumps(config))
                    subprocess.run(
                        [
                            "cargo",
                            "run",
                            "--locked",
                            "--example",
                            "mcp_capability_spike",
                            "--",
                            str(path),
                        ],
                        cwd=args.htlk,
                        check=True,
                        timeout=300,
                    )
            finally:
                if process is not None:
                    process.terminate()
                    process.wait(timeout=15)
    finally:
        mock.shutdown()
        mock.server_close()


if __name__ == "__main__":
    main()
