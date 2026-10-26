"""Deployment invariants that schema validation alone cannot establish."""

from pathlib import Path
import tomllib
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[1]


class DeploymentTests(unittest.TestCase):
    def test_oauth_rollout_has_single_writer_and_persistent_state(self):
        resources = list(
            yaml.safe_load_all((ROOT / "deploy/kubernetes/oauth.yaml").read_text())
        )
        deployment = next(
            resource for resource in resources if resource["kind"] == "Deployment"
        )
        self.assertEqual(deployment["spec"]["replicas"], 1)
        self.assertEqual(deployment["spec"]["strategy"]["type"], "Recreate")
        pod = deployment["spec"]["template"]["spec"]
        self.assertIn("persistentVolumeClaim", pod["volumes"][0])
        self.assertFalse(pod["containers"][0]["volumeMounts"][0].get("readOnly", False))
        self.assertTrue(pod["securityContext"]["runAsNonRoot"])
        self.assertEqual(
            pod["securityContext"]["fsGroupChangePolicy"], "OnRootMismatch"
        )
        self.assertIn("chmod 600", pod["initContainers"][0]["args"][0])

    def test_versioned_images_match_package_and_sessions_have_affinity(self):
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
        for path in (ROOT / "deploy/kubernetes").glob("*.yaml"):
            for resource in yaml.safe_load_all(path.read_text()):
                if resource["kind"] == "Service":
                    self.assertEqual(resource["spec"]["sessionAffinity"], "ClientIP")
                if resource["kind"] in ("Deployment", "Job"):
                    pod = resource["spec"]["template"]["spec"]
                    for container in pod["containers"] + pod.get("initContainers", []):
                        self.assertTrue(container["image"].endswith(f":{version}"))
                        self.assertTrue(
                            container["securityContext"]["readOnlyRootFilesystem"]
                        )


if __name__ == "__main__":
    unittest.main()
