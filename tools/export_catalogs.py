#!/usr/bin/env python3
"""Regenerate or verify version-bound HTLK catalogs from the compiled binary."""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    for transport in ("stdio", "streamable_http"):
        output = subprocess.check_output(
            ["cargo", "run", "--quiet", "--locked", "--", "catalog", "--htlk", "--transport", transport],
            cwd=ROOT,
            text=True,
        )
        json.loads(output)
        path = ROOT / "examples" / "htlk" / f"{transport}.catalogs.json"
        if args.check:
            if path.read_text() != output:
                raise SystemExit(f"catalog drift: run python3 tools/export_catalogs.py ({path})")
        else:
            path.write_text(output)


if __name__ == "__main__":
    main()
