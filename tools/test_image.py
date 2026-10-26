"""Registry failures must never be interpreted as permission to replace an image."""

import subprocess
import unittest
from unittest.mock import patch

import publish_image


class ImageTests(unittest.TestCase):
    def test_registry_errors_fail_closed(self):
        for message in (
            "authentication failed",
            "connection timed out",
            "credential token not found",
        ):
            with patch(
                "subprocess.run",
                return_value=subprocess.CompletedProcess([], 1, "", message),
            ):
                with self.assertRaises(RuntimeError):
                    publish_image.digest("registry/image:1.2.3")
        with patch(
            "subprocess.run",
            return_value=subprocess.CompletedProcess([], 1, "", "manifest unknown"),
        ):
            self.assertIsNone(publish_image.digest("registry/image:1.2.3"))

    def test_existing_version_never_triggers_push(self):
        environment = {
            "GITHUB_REPOSITORY": "owner/llm-mcp",
            "GITHUB_SHA": "a" * 40,
            "RELEASE_VERSION": "1.2.3",
        }
        with (
            patch.dict("os.environ", environment),
            patch("publish_image.digest", side_effect=["digest-one", "digest-two"]),
            patch("subprocess.run") as run,
            patch("publish_image.load_receipt", return_value={"images": {}}),
        ):
            with self.assertRaises(ValueError):
                publish_image.main()
            run.assert_not_called()

    def test_interrupted_index_aliases_resume_without_rebuilding_or_replacing(self):
        environment = {
            "GITHUB_REPOSITORY": "owner/llm-mcp",
            "GITHUB_SHA": "a" * 40,
            "RELEASE_VERSION": "1.2.3",
        }
        for version, commit, missing in [
            ("digest", None, ":sha-"),
            (None, "digest", ":1.2.3"),
            ("digest", "digest", None),
        ]:
            with (
                patch.dict("os.environ", environment),
                patch(
                    "publish_image.load_receipt",
                    return_value={"images": {"amd64": {"config_id": "verified"}}},
                ),
                patch("publish_image.digest", side_effect=[version, commit]),
                patch("publish_image.verify_remote") as verify,
                patch("publish_image.alias") as alias,
                patch("subprocess.run") as run,
            ):
                publish_image.main()
                verify.assert_called_once()
                run.assert_not_called()
                if missing:
                    self.assertIn(missing, alias.call_args.args[1])
                    self.assertEqual(alias.call_args.args[2], "digest")
                    self.assertIn("@digest", alias.call_args.args[0])
                else:
                    alias.assert_not_called()

    def test_resumed_registry_image_must_match_original_receipt(self):
        with patch("publish_image.remote_config", return_value="changed-image"):
            with self.assertRaises(ValueError):
                publish_image.verify_remote(
                    "image@digest", {"amd64": {"config_id": "original-image"}}
                )

    def test_fresh_publication_checks_native_configs_and_creates_commit_before_version(
        self,
    ):
        environment = {
            "GITHUB_REPOSITORY": "owner/llm-mcp",
            "GITHUB_SHA": "a" * 40,
            "RELEASE_VERSION": "1.2.3",
        }
        images = {
            "amd64": {"config_id": "native-amd"},
            "arm64": {"config_id": "native-arm"},
        }
        with (
            patch.dict("os.environ", environment),
            patch("publish_image.load_receipt", return_value={"images": images}),
            patch(
                "publish_image.loaded_image_contents", side_effect=list(images.values())
            ),
            patch(
                "publish_image.digest",
                side_effect=[None, None, None, None, "index-digest"],
            ),
            patch("publish_image.verify_remote") as verify,
            patch("publish_image.alias") as alias,
            patch("subprocess.run") as run,
        ):
            publish_image.main()
            index = run.call_args.args[0]
            self.assertEqual(index[:4], ["docker", "buildx", "imagetools", "create"])
            self.assertIn(":sha-", index[5])
            self.assertNotIn("ghcr.io/owner/llm-mcp:1.2.3", index)
            alias.assert_called_once_with(
                "ghcr.io/owner/llm-mcp@index-digest",
                "ghcr.io/owner/llm-mcp:1.2.3",
                "index-digest",
            )
            verify.assert_called_once()


if __name__ == "__main__":
    unittest.main()
