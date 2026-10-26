#!/usr/bin/env python3
"""Publish tested native images, refusing replacement of an existing version."""

import os
import re
import subprocess
import json
from pathlib import Path

from release import loaded_image_contents, verify_receipt


def digest(image):
    result = subprocess.run(
        ["docker", "buildx", "imagetools", "inspect", image],
        capture_output=True,
        text=True,
        timeout=90,
    )
    if result.returncode:
        if "manifest unknown" in result.stderr.lower() or re.search(
            r": not found\s*$", result.stderr.lower()
        ):
            return None
        raise RuntimeError(result.stderr)
    match = re.search(r"^Digest:\s+(sha256:[a-f0-9]{64})", result.stdout, re.MULTILINE)
    if not match:
        raise RuntimeError("Registry returned no image-index digest")
    return match[1]


def load_receipt(version, revision):
    directory = Path("dist")
    receipt = json.loads((directory / "receipt.json").read_text())
    verify_receipt(receipt, directory, version, revision)
    return receipt


def remote_config(reference, arch):
    manifest = json.loads(
        subprocess.check_output(
            ["docker", "buildx", "imagetools", "inspect", reference, "--raw"],
            text=True,
            timeout=90,
        )
    )
    if "config" in manifest:
        return manifest["config"]["digest"]
    matches = [
        item
        for item in manifest["manifests"]
        if item.get("platform", {}).get("os") == "linux"
        and item.get("platform", {}).get("architecture") == arch
    ]
    if len(matches) != 1:
        raise ValueError(
            "Registry index does not have exactly one expected native platform"
        )
    repository = reference.split("@")[0].rsplit(":", 1)[0]
    return remote_config(f"{repository}@{matches[0]['digest']}", arch)


def verify_remote(reference, images):
    for arch, expected in images.items():
        if remote_config(reference, arch) != expected["config_id"]:
            raise ValueError(
                "Registry image differs from the immutable tested artifact set"
            )


def alias(source, destination, expected_digest):
    subprocess.run(
        ["docker", "buildx", "imagetools", "create", "-t", destination, source],
        check=True,
    )
    if digest(destination) != expected_digest:
        raise ValueError("Registry alias did not preserve the verified image index")


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
    receipt = load_receipt(version, revision)
    published, committed = digest(immutable), digest(commit)
    if published or committed:
        if published and committed and published != committed:
            raise ValueError(
                "Published version does not match the immutable commit tag"
            )
        existing = published or committed
        source = f"{image}@{existing}"
        verify_remote(source, receipt["images"])
        if not published:
            alias(source, immutable, existing)
        if not committed:
            alias(source, commit, existing)
        print(f"Verified immutable release: {immutable}@{existing}")
        return
    sources = []
    for arch in ("amd64", "arm64"):
        subprocess.run(["docker", "load", "-i", f"dist/image-{arch}.tar"], check=True)
        actual = loaded_image_contents("llm-mcp:release")
        if actual != receipt["images"][arch]:
            raise ValueError("Loaded native image differs from release receipt")
        tag = f"{image}:sha-{revision}-{arch}"
        if digest(tag):
            verify_remote(tag, {arch: receipt["images"][arch]})
        else:
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
            commit,
            *sources,
        ],
        check=True,
    )
    committed = digest(commit)
    if not committed:
        raise ValueError("Registry did not publish the commit index")
    source = f"{image}@{committed}"
    verify_remote(source, receipt["images"])
    alias(source, immutable, committed)
    print(f"Published: {immutable}@{committed}")


if __name__ == "__main__":
    main()
