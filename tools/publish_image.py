#!/usr/bin/env python3
"""Publish tested native images, refusing replacement of an existing version."""

import os
import re
import subprocess


def digest(image):
    result = subprocess.run(
        ["docker", "buildx", "imagetools", "inspect", image],
        capture_output=True,
        text=True,
        timeout=90,
    )
    if result.returncode:
        if any(
            message in result.stderr.lower()
            for message in ("manifest unknown", "not found")
        ):
            return None
        raise RuntimeError(result.stderr)
    match = re.search(r"^Digest:\s+(sha256:[a-f0-9]{64})", result.stdout, re.MULTILINE)
    if not match:
        raise RuntimeError("Registry returned no image-index digest")
    return match[1]


def main():
    image = f"ghcr.io/{os.environ['GITHUB_REPOSITORY'].lower()}"
    version = os.environ["RELEASE_VERSION"]
    revision = os.environ["GITHUB_SHA"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version) or not re.fullmatch(
        r"[a-f0-9]{40}", revision
    ):
        raise ValueError("Invalid version or commit")
    immutable = f"{image}:{version}"
    commit = f"{image}:sha-{revision}"
    existing = digest(immutable)
    if existing:
        if digest(commit) != existing:
            raise ValueError(
                "Published version does not match the immutable commit tag"
            )
        print(f"Already published: {immutable}@{existing}")
        return
    sources = []
    for arch in ("amd64", "arm64"):
        subprocess.run(["docker", "load", "-i", f"dist/image-{arch}.tar"], check=True)
        tag = f"{image}:sha-{revision}-{arch}"
        subprocess.run(["docker", "tag", "llm-mcp:release", tag], check=True)
        subprocess.run(["docker", "push", tag], check=True)
        sources.append(tag)
    subprocess.run(
        [
            "docker",
            "buildx",
            "imagetools",
            "create",
            "-t",
            immutable,
            "-t",
            commit,
            *sources,
        ],
        check=True,
    )
    print(f"Published: {immutable}@{digest(immutable)}")


if __name__ == "__main__":
    main()
