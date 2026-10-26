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
import shutil

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
    path.with_name(f"{path.name}.sha256").write_text(
        f"{checksum}  {path.name}\n", newline="\n"
    )
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


def release_assets(tag):
    """Find or create the unpublished release staging area."""
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
        return {}
    else:
        return {asset["name"]: asset for asset in json.loads(result.stdout)["assets"]}


def file_hash(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def archive_name(package_version, target):
    suffix = "zip" if "windows" in target else "tar.gz"
    return f"llm-mcp-{package_version}-{target}.{suffix}"


def expected_assets(package_version):
    archives = {archive_name(package_version, target) for target in TARGETS}
    return (
        archives
        | {f"{name}.sha256" for name in archives}
        | {"image-amd64.tar", "image-arm64.tar", f"llm-mcp-{package_version}.crate"}
    )


def image_contents(path):
    """Read image config identity and the actual executable from Docker save layers."""
    with tarfile.open(path) as image:
        manifests = json.load(image.extractfile("manifest.json"))
        if len(manifests) != 1:
            raise ValueError("Expected one native image in each Docker archive")
        manifest = manifests[0]
        config = image.extractfile(manifest["Config"]).read()
        binary_hash = None
        for name in reversed(manifest["Layers"]):
            with tarfile.open(fileobj=image.extractfile(name), mode="r|*") as layer:
                for member in layer:
                    path_in_image = member.name.removeprefix("./")
                    if path_in_image == "usr/local/bin/.wh.llm-mcp":
                        raise ValueError("Image removes the server executable")
                    if path_in_image == "usr/local/bin/llm-mcp":
                        if not member.isfile():
                            raise ValueError(
                                "Server executable must be a regular image file"
                            )
                        binary_hash = hashlib.file_digest(
                            layer.extractfile(member), "sha256"
                        ).hexdigest()
                        break
            if binary_hash:
                break
        if not binary_hash:
            raise ValueError("Image has no server executable")
        return {
            "config_id": f"sha256:{hashlib.sha256(config).hexdigest()}",
            "architecture": json.loads(config)["architecture"],
            "binary_sha256": binary_hash,
        }


def verify_native_image(image_path, binary_path, arch):
    image = image_contents(image_path)
    if image["architecture"] != arch:
        raise ValueError("Wrong image architecture")
    with tarfile.open(binary_path) as binary:
        member = next(item for item in binary if item.name.endswith("/llm-mcp"))
        actual = hashlib.file_digest(binary.extractfile(member), "sha256").hexdigest()
    if actual != image["binary_sha256"]:
        raise ValueError(
            "Linux binary archive differs from the tested container executable"
        )
    return image


def loaded_image_contents(image):
    # Containerd-backed Docker may report an OCI index as image inspect .Id.
    # Read the native config blob instead of assuming .Id is its config digest.
    with tempfile.TemporaryDirectory(prefix="llm-mcp-loaded-image-") as temporary:
        path = Path(temporary) / "image.tar"
        subprocess.run(["docker", "save", "--output", str(path), image], check=True)
        return image_contents(path)


def build_receipt(directory, package_version, revision):
    files = {
        name: file_hash(directory / name)
        for name in sorted(expected_assets(package_version))
    }
    for target in TARGETS:
        name = archive_name(package_version, target)
        if (
            directory / f"{name}.sha256"
        ).read_text().strip() != f"{files[name]}  {name}":
            raise ValueError("Binary archive checksum manifest does not match")
    images = {}
    for arch, target in (
        ("amd64", "x86_64-unknown-linux-gnu"),
        ("arm64", "aarch64-unknown-linux-gnu"),
    ):
        images[arch] = verify_native_image(
            directory / f"image-{arch}.tar",
            directory / archive_name(package_version, target),
            arch,
        )
    return {
        "version": package_version,
        "commit": revision,
        "files": files,
        "images": images,
    }


def verify_receipt(receipt, directory, package_version, revision):
    if receipt["version"] != package_version or receipt["commit"] != revision:
        raise ValueError("Release receipt belongs to a different version or commit")
    if set(receipt["files"]) != expected_assets(package_version):
        raise ValueError("Incomplete or unexpected release artifact set")
    for name, expected in receipt["files"].items():
        if file_hash(directory / name) != expected:
            raise ValueError(f"Release artifact changed: {name}")


def download_asset(tag, name, directory):
    subprocess.run(
        ["gh", "release", "download", tag, "--pattern", name, "--dir", str(directory)],
        check=True,
    )


def upload_verified(tag, path, existing):
    if path.name in existing:
        with tempfile.TemporaryDirectory() as temporary:
            download_asset(tag, path.name, Path(temporary))
            if file_hash(Path(temporary) / path.name) != file_hash(path):
                raise ValueError(
                    f"Existing release asset differs: {path.name}; resume with the original build artifacts"
                )
    else:
        subprocess.run(["gh", "release", "upload", tag, str(path)], check=True)


def prepare_release(directory, package_version, revision):
    """Freeze one artifact set before any registry publication; reuse it on retries."""
    tag = f"v{package_version}"
    existing = release_assets(tag)
    if "receipt.json" in existing:
        with tempfile.TemporaryDirectory() as temporary:
            original = Path(temporary)
            download_asset(tag, "receipt.json", original)
            receipt = json.loads((original / "receipt.json").read_text())
            if (
                receipt["version"] != package_version
                or receipt["commit"] != revision
                or set(receipt["files"]) != expected_assets(package_version)
            ):
                raise ValueError(
                    "Existing immutable release receipt does not match this release"
                )
            for name in receipt["files"]:
                download_asset(tag, name, original)
            verify_receipt(receipt, original, package_version, revision)
            for name in [*receipt["files"], "receipt.json"]:
                shutil.copyfile(original / name, directory / name)
    else:
        crate = ROOT / "target/package" / f"llm-mcp-{package_version}.crate"
        shutil.copyfile(crate, directory / crate.name)
        receipt = build_receipt(directory, package_version, revision)
        for name in receipt["files"]:
            upload_verified(tag, directory / name, existing)
        path = directory / "receipt.json"
        path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        upload_verified(tag, path, existing)
    verify_receipt(receipt, directory, package_version, revision)


def github_release(tag, directory):
    """Finalize only the frozen, verified artifact set after registry jobs succeed."""
    receipt = json.loads((directory / "receipt.json").read_text())
    verify_receipt(receipt, directory, version(), os.environ["GITHUB_SHA"])
    with tempfile.TemporaryDirectory() as temporary:
        download_asset(tag, "receipt.json", Path(temporary))
        if file_hash(Path(temporary) / "receipt.json") != file_hash(
            directory / "receipt.json"
        ):
            raise ValueError("Remote release receipt changed")
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
    image = commands.add_parser("verify-image")
    image.add_argument("--image-archive", type=Path, required=True)
    image.add_argument("--binary-archive", type=Path, required=True)
    image.add_argument("--arch", choices=["amd64", "arm64"], required=True)
    loaded = commands.add_parser("verify-loaded-image")
    loaded.add_argument("--image", required=True)
    commands.add_parser("publish-crate")
    prepare = commands.add_parser("prepare")
    prepare.add_argument("--directory", type=Path, default=ROOT / "dist")
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
    elif args.command == "verify-image":
        print(
            json.dumps(
                verify_native_image(args.image_archive, args.binary_archive, args.arch),
                indent=2,
            )
        )
    elif args.command == "verify-loaded-image":
        print(json.dumps(loaded_image_contents(args.image), indent=2))
    elif args.command == "publish-crate":
        directory = ROOT / "dist"
        receipt = json.loads((directory / "receipt.json").read_text())
        verify_receipt(receipt, directory, package_version, os.environ["GITHUB_SHA"])
        prepared = directory / f"llm-mcp-{package_version}.crate"
        if not published_crate_matches(package_version, prepared):
            subprocess.run(["cargo", "package", "--locked"], check=True)
            crate = ROOT / "target/package" / prepared.name
            if file_hash(crate) != file_hash(prepared):
                raise ValueError(
                    "Repackaged crate differs from the immutable release set"
                )
            subprocess.run(["cargo", "publish", "--locked"], check=True)
    elif args.command == "prepare":
        prepare_release(args.directory, package_version, os.environ["GITHUB_SHA"])
    elif args.command == "github":
        github_release(f"v{package_version}", args.directory)


if __name__ == "__main__":
    main()
