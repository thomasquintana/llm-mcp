#!/usr/bin/env python3
"""Fail if third-party notices drift from the locked dependency graph."""

from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    with tempfile.TemporaryDirectory(prefix="llm-mcp-notices-") as directory:
        path = Path(directory) / "notices"
        subprocess.run(
            [
                "cargo",
                "about",
                "generate",
                "about.hbs",
                "--locked",
                "--fail",
                "--output-file",
                str(path),
            ],
            cwd=ROOT,
            check=True,
        )
        if path.read_bytes() != (ROOT / "THIRD_PARTY_LICENSES").read_bytes():
            raise SystemExit(
                "Notices drift: cargo about generate about.hbs --locked --fail --output-file THIRD_PARTY_LICENSES"
            )


if __name__ == "__main__":
    main()
