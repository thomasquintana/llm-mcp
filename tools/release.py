#!/usr/bin/env python3
"""Version validation and binary archives shared by release CI and local dry runs."""

import argparse
import hashlib
import gzip
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.error
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
}


def version():
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


def validate_ref(ref, package_version, dry_run):
    if not re.fullmatch(r"\d+\.\d+\.\d+", package_version):
        raise ValueError("Release package version must be MAJOR.MINOR.PATCH")
    if not dry_run and ref != f"v{package_version}":
        raise ValueError(
            "Publishing requires an exact vMAJOR.MINOR.PATCH tag matching Cargo.toml"
        )
    if ref.startswith("v") and ref != f"v{package_version}":
        raise ValueError("Tag/package version mismatch")
    return f"v{package_version}"


def archive(binary, target, output, package_version):
    if target not in TARGETS:
        raise ValueError("Unsupported release target")
    output.mkdir(parents=True, exist_ok=True)
    prefix = f"llm-mcp-{package_version}-{target}"
    files = [
        (binary, "llm-mcp.exe" if "windows" in target else "llm-mcp"),
        (ROOT / "LICENSE", "LICENSE"),
        (ROOT / "THIRD_PARTY_LICENSES", "THIRD_PARTY_LICENSES"),
        (ROOT / "README.md", "README.md"),
    ]
    if "windows" in target:
        path = output / f"{prefix}.zip"
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as bundle:
            for source, name in files:
                info = zipfile.ZipInfo(
                    f"{prefix}/{name}", date_time=(1980, 1, 1, 0, 0, 0)
                )
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (0o755 if name == "llm-mcp.exe" else 0o644) << 16
                bundle.writestr(info, source.read_bytes())
    else:
        path = output / f"{prefix}.tar.gz"

        def normalize(info):
            info.mtime = info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mode = 0o755 if info.name.endswith("/llm-mcp") else 0o644
            return info

        with (
            path.open("wb") as raw,
            gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as compressed,
        ):
            with tarfile.open(fileobj=compressed, mode="w") as bundle:
                for source, name in files:
                    bundle.add(source, arcname=f"{prefix}/{name}", filter=normalize)
    checksum = hashlib.sha256(path.read_bytes()).hexdigest()
    path.with_name(f"{path.name}.sha256").write_text(f"{checksum}  {path.name}\n")
    return path


def published_crate_matches(package_version, crate_path):
    request = urllib.request.Request(
        f"https://crates.io/api/v1/crates/llm-mcp/{package_version}",
        headers={"User-Agent": "llm-mcp-release"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            published = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return False
        raise
    actual = hashlib.sha256(crate_path.read_bytes()).hexdigest()
    if published["version"]["checksum"] != actual:
        raise ValueError(
            "Published crate has different contents; never overwrite a version"
        )
    return True


def github_release(tag, directory):
    """Resume without overwriting existing release assets or publishing early."""
    repo = os.environ["GITHUB_REPOSITORY"]
    result = subprocess.run(
        ["gh", "api", f"repos/{repo}/releases/tags/{tag}"],
        capture_output=True,
        text=True,
    )
    if result.returncode:
        if "404" not in result.stderr:
            raise RuntimeError(result.stderr)
        subprocess.run(
            [
                "gh",
                "release",
                "create",
                tag,
                "--verify-tag",
                "--draft",
                "--generate-notes",
            ],
            check=True,
        )
        existing = {}
    else:
        existing = {
            asset["name"]: asset for asset in json.loads(result.stdout)["assets"]
        }
    assets = sorted(
        path
        for path in directory.iterdir()
        if path.name.endswith((".tar.gz", ".zip", ".sha256"))
    )
    if len([path for path in assets if path.name.endswith((".tar.gz", ".zip"))]) != len(
        TARGETS
    ):
        raise ValueError("All five binary archives are required")
    for path in assets:
        if path.name in existing:
            with tempfile.TemporaryDirectory() as temp:
                subprocess.run(
                    [
                        "gh",
                        "release",
                        "download",
                        tag,
                        "--pattern",
                        path.name,
                        "--dir",
                        temp,
                    ],
                    check=True,
                )
                if (Path(temp) / path.name).read_bytes() != path.read_bytes():
                    raise ValueError(f"Existing release asset differs: {path.name}")
        else:
            subprocess.run(["gh", "release", "upload", tag, str(path)], check=True)
    subprocess.run(["gh", "release", "edit", tag, "--draft=false"], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate")
    validate.add_argument("--ref", required=True)
    validate.add_argument("--dry-run", action="store_true")
    bundle = commands.add_parser("archive")
    bundle.add_argument("--binary", type=Path, required=True)
    bundle.add_argument("--target", required=True)
    bundle.add_argument("--output", type=Path, default=ROOT / "dist")
    commands.add_parser("publish-crate")
    github = commands.add_parser("github")
    github.add_argument("--directory", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    package_version = version()
    if args.command == "validate":
        tag = validate_ref(args.ref, package_version, args.dry_run)
        if not args.dry_run:
            if os.environ.get("GITHUB_REF_TYPE") != "tag":
                raise ValueError("Publishing requires a tag ref")
            subprocess.run(
                ["git", "merge-base", "--is-ancestor", "HEAD", "origin/main"],
                check=True,
            )
        values = f"version={package_version}\ntag={tag}\n"
        print(values, end="")
        if "GITHUB_OUTPUT" in os.environ:
            with open(os.environ["GITHUB_OUTPUT"], "a") as output:
                output.write(values)
    elif args.command == "archive":
        print(archive(args.binary, args.target, args.output, package_version))
    elif args.command == "publish-crate":
        subprocess.run(["cargo", "package", "--locked"], check=True)
        crate = ROOT / "target/package" / f"llm-mcp-{package_version}.crate"
        if not published_crate_matches(package_version, crate):
            subprocess.run(["cargo", "publish", "--locked"], check=True)
    elif args.command == "github":
        github_release(f"v{package_version}", args.directory)


if __name__ == "__main__":
    main()
