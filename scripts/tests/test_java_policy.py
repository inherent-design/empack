import hashlib
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("java_policy_update", ROOT / "scripts/update-java-policy.py")
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)


class JavaPolicyRefresh(unittest.TestCase):
    def fixture(self, majors=(16, 17)):
        profile = json.dumps({"compatibleJavaMajors": list(majors)}).encode()
        digest = hashlib.sha256(profile).hexdigest()
        index = json.dumps({"versions": [{"version": "1.17.1", "type": "release", "sha256": digest}]}).encode()
        return profile, index

    def test_refresh_binds_each_profile_to_the_pinned_index(self):
        profile, index = self.fixture()
        with patch.object(policy, "SNAPSHOTS", set()), patch.object(policy, "receive", side_effect=lambda url: index if url.endswith("index.json") else profile):
            generated = policy.generate(policy.REVISION)
        self.assertIn("1.17.1|16|16,17|" + hashlib.sha256(profile).hexdigest(), generated)
        self.assertIn("index.json sha256 " + hashlib.sha256(index).hexdigest(), generated)

    def test_corrupt_or_ambiguous_profiles_do_not_produce_policy(self):
        profile, index = self.fixture()
        with patch.object(policy, "SNAPSHOTS", set()), patch.object(policy, "receive", side_effect=lambda url: index if url.endswith("index.json") else b"corrupt"):
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                policy.generate(policy.REVISION)
        for majors in [(), (17, 16), (16, 16), (7,), (True,), (65536,)]:
            profile, index = self.fixture(majors)
            with self.subTest(majors=majors), patch.object(policy, "SNAPSHOTS", set()), patch.object(policy, "receive", side_effect=lambda url: index if url.endswith("index.json") else profile):
                with self.assertRaisesRegex(ValueError, "Unsupported Java profile"):
                    policy.generate(policy.REVISION)

    def test_history_and_sparse_support_do_not_invent_compatibility(self):
        self.assertEqual(policy.historical_minimum("1.5.2", [8]), 5)
        self.assertEqual(policy.historical_minimum("1.11.2", [8]), 6)
        self.assertEqual(policy.historical_minimum("1.12", [8]), 8)
        self.assertEqual(policy.historical_minimum("26.1", [25]), 25)
        profile, index = self.fixture((17, 21))
        with patch.object(policy, "SNAPSHOTS", set()), patch.object(policy, "receive", side_effect=lambda url: index if url.endswith("index.json") else profile):
            generated = policy.generate(policy.REVISION)
        self.assertIn("|17,21|", generated)
        self.assertNotIn("17,18", generated)
        with self.assertRaisesRegex(ValueError, "exact metadata commit"):
            policy.generate("master")


if __name__ == "__main__":
    unittest.main()
