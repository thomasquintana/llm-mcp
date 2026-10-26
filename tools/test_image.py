"""Registry failures must never be interpreted as permission to replace an image."""

import subprocess
import unittest
from unittest.mock import patch

import publish_image


class ImageTests(unittest.TestCase):
    def test_registry_errors_fail_closed(self):
        for message in ("authentication failed", "connection timed out"):
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
        ):
            with self.assertRaises(ValueError):
                publish_image.main()
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
