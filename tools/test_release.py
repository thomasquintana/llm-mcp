"""Regression coverage for irreversible publishing boundaries and archive contents."""

import hashlib
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile
from unittest.mock import patch
import urllib.error
import io
import json
import gzip

import release


class ReleaseTests(unittest.TestCase):
    def test_rebuilt_files_are_replaced_with_the_original_frozen_release_set(self):
        package_version = "1.2.3"
        revision = "a" * 40
        files = {
            name: f"original-{name}".encode()
            for name in release.expected_assets(package_version)
        }
        receipt = {
            "version": package_version,
            "commit": revision,
            "files": {
                name: hashlib.sha256(data).hexdigest() for name, data in files.items()
            },
            "images": {},
        }
        files["receipt.json"] = json.dumps(receipt).encode()

        def download(_tag, name, directory):
            (directory / name).write_bytes(files[name])

        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for name in receipt["files"]:
                (directory / name).write_bytes(b"different-rebuilt-content")
            with (
                patch("release.release_assets", return_value={"receipt.json": {}}),
                patch("release.download_asset", side_effect=download),
                patch("subprocess.run") as run,
            ):
                release.prepare_release(directory, package_version, revision)
                run.assert_not_called()
            for name, data in files.items():
                self.assertEqual((directory / name).read_bytes(), data)

    def test_receipt_binds_container_executable_to_binary_archive_and_rejects_drift(
        self,
    ):
        def docker_image(path, architecture, binary, compressed=False):
            layer = io.BytesIO()
            with tarfile.open(fileobj=layer, mode="w") as archive:
                info = tarfile.TarInfo("usr/local/bin/llm-mcp")
                info.size = len(binary)
                archive.addfile(info, io.BytesIO(binary))
            files = {
                "config.json": json.dumps({"architecture": architecture}).encode(),
                "layer.tar": gzip.compress(layer.getvalue())
                if compressed
                else layer.getvalue(),
                "manifest.json": json.dumps(
                    [{"Config": "config.json", "Layers": ["layer.tar"]}]
                ).encode(),
            }
            with tarfile.open(path, "w") as image:
                for name, data in files.items():
                    info = tarfile.TarInfo(name)
                    info.size = len(data)
                    image.addfile(info, io.BytesIO(data))

        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            binary = directory / "binary"
            binary.write_bytes(b"tested-binary")
            for target in release.TARGETS:
                release.archive(binary, target, directory, "1.2.3")
            (directory / "llm-mcp-1.2.3.crate").write_bytes(b"crate")
            for arch in ("amd64", "arm64"):
                docker_image(
                    directory / f"image-{arch}.tar",
                    arch,
                    b"tested-binary",
                    compressed=arch == "arm64",
                )
            receipt = release.build_receipt(directory, "1.2.3", "a" * 40)
            release.verify_receipt(receipt, directory, "1.2.3", "a" * 40)
            with self.assertRaises(ValueError):
                release.verify_receipt(receipt, directory, "1.2.3", "b" * 40)
            docker_image(directory / "image-arm64.tar", "arm64", b"different-build")
            with self.assertRaises(ValueError):
                release.build_receipt(directory, "1.2.3", "a" * 40)
            with self.assertRaises(ValueError):
                release.verify_receipt(receipt, directory, "1.2.3", "a" * 40)

    def test_crate_resume_distinguishes_missing_transient_and_different_contents(self):
        with tempfile.TemporaryDirectory() as temp:
            crate = Path(temp) / "package.crate"
            crate.write_bytes(b"immutable-package")
            for status in (404, 403, 500):
                error = urllib.error.HTTPError(
                    "https://crates.io", status, "error", {}, None
                )
                with patch("urllib.request.urlopen", side_effect=error):
                    if status == 404:
                        self.assertFalse(
                            release.published_crate_matches("1.2.3", crate)
                        )
                    else:
                        with self.assertRaises(urllib.error.HTTPError):
                            release.published_crate_matches("1.2.3", crate)
            for checksum in (
                hashlib.sha256(crate.read_bytes()).hexdigest(),
                "different",
            ):
                body = io.BytesIO(
                    json.dumps({"version": {"checksum": checksum}}).encode()
                )
                with patch("urllib.request.urlopen", return_value=body):
                    if checksum == "different":
                        with self.assertRaises(ValueError):
                            release.published_crate_matches("1.2.3", crate)
                    else:
                        self.assertTrue(release.published_crate_matches("1.2.3", crate))

    def test_publish_requires_matching_strict_version(self):
        self.assertEqual(release.validate_ref("v1.2.3", "1.2.3", False), "v1.2.3")
        self.assertEqual(release.validate_ref("main", "1.2.3", True), "v1.2.3")
        for ref, version in [
            ("main", "1.2.3"),
            ("v1.2.4", "1.2.3"),
            ("v1.2.3-rc1", "1.2.3"),
            ("v1.2.3", "1.2.3-rc1"),
        ]:
            with self.assertRaises(ValueError):
                release.validate_ref(ref, version, False)

    def test_archives_include_licenses_and_matching_checksums(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            binary = directory / "binary"
            binary.write_bytes(b"test-binary")
            binary.chmod(0o755)
            for target in release.TARGETS:
                path = release.archive(binary, target, directory / "dist", "1.2.3")
                digest = hashlib.sha256(path.read_bytes()).hexdigest()
                repeated = release.archive(binary, target, directory / "again", "1.2.3")
                self.assertEqual(path.read_bytes(), repeated.read_bytes())
                self.assertTrue(
                    path.with_name(f"{path.name}.sha256").read_text().startswith(digest)
                )
                self.assertNotIn(
                    b"\r", path.with_name(f"{path.name}.sha256").read_bytes()
                )
                if path.suffix == ".zip":
                    with zipfile.ZipFile(path) as bundle:
                        names = bundle.namelist()
                else:
                    with tarfile.open(path) as bundle:
                        names = bundle.getnames()
                        executable = next(
                            item
                            for item in bundle.getmembers()
                            if item.name.endswith("/llm-mcp")
                        )
                        self.assertTrue(executable.mode & 0o111)
                self.assertTrue(any(name.endswith("/LICENSE") for name in names))
                self.assertTrue(
                    any(name.endswith("/THIRD_PARTY_LICENSES") for name in names)
                )
            with self.assertRaises(ValueError):
                release.archive(binary, "../escape", directory, "1.2.3")


if __name__ == "__main__":
    unittest.main()
