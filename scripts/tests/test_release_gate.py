"""Synthetic evidence chains test admission; they are not release execution proof."""
import copy
import json
from pathlib import Path
import re
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import test_ci_verification as ci_fixtures
import test_release_inputs as input_fixtures
from verification.evidence import sha256, write_json
from verification import release
from verification.consumer import INPUTS
from verification.python_tests import PRODUCER_INPUTS


class ReleaseArchiveGate(unittest.TestCase):
    def git(self, *args):
        return release.git(self.root, *args)

    def setUp(self):
        input_fixtures.ReleaseInputs.setUp(self)
        registry = self.root / "tests/verification"
        registry.mkdir(parents=True)
        for name in ("requirements.json", "test-bindings.json", "python-bindings.json"):
            (registry / name).write_bytes((ci_fixtures.SCRIPTS.parent / "tests/verification" / name).read_bytes())
        for name in (*INPUTS, *PRODUCER_INPUTS):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((ci_fixtures.SCRIPTS.parent / name).read_bytes())
        self.git("add", "tests", "scripts", "crates")
        self.git("commit", "-qm", "fixture registry")
        self.revision = self.git("rev-parse", "HEAD")
        self.version = "0.8.4-beta"
        self.inputs = input_fixtures.release.prepare(self.root, self.version)
        self.git("restore", "Cargo.toml", "Cargo.lock")
        self.artifacts = self.root / ".git/artifacts"
        self.artifacts.mkdir()
        self.output = self.root / ".git/publish"
        self.paths = {}
        for target in release.TARGETS:
            build_dir = self.artifacts / f"release-build-{target}"
            build_dir.mkdir()
            archive = build_dir / release.archive_name(self.version, target)
            archive.write_bytes(b"inert fixture bytes; not a distribution")
            executable = {"sha256": "c" * 64, "length": 17}
            build = {"schema": 1, "kind": "release-archive-build", "version": self.version,
                     "source_revision": self.revision, "target": target, "features": "default",
                     "profile": "release", "inputs": self.inputs,
                     "compiler": f"rustc fixture\nrelease: 1.99.1\nhost: {target}",
                     "command": ["cargo", "build", "--locked", "--release", "-p", "empack", "--target", target],
                     "archive": {"name": archive.name, "sha256": sha256(archive), "length": archive.stat().st_size},
                     "executable": executable}
            write_json(build_dir / "build.json", build)
            execution = self.artifacts / f"release-test-{target}" / "package-attempt"
            with patch.object(ci_fixtures, "REVISION", self.revision):
                attempt, receipt = ci_fixtures.fixture_attempt(execution / "suite", target, "default", ["cli"])
            receipt["artifact"]["version"] = self.version
            receipt["artifact"]["profile"] = "release"
            write_json(attempt / "receipt.json", receipt)
            package = {"schema": 1, "kind": "extracted-release-archive", "status": "PASS",
                       "version": self.version, "target": target, "features": "default", "profile": "release",
                       "test_source_revision": self.revision, "test_worktree_dirty": False,
                       "test_source_snapshot_sha256": receipt["source_snapshot_sha256"],
                       "archive": {"name": archive.name, "sha256": sha256(archive)}, "executable": executable,
                       "suite_receipt": (attempt / "receipt.json").relative_to(execution).as_posix(),
                       "suite_receipt_sha256": sha256(attempt / "receipt.json")}
            write_json(execution / "package.json", package)
            self.paths[target] = (build_dir, execution, attempt)

    def admit(self):
        return release.admit_archives(self.root, self.artifacts, self.version, self.revision, self.output)

    def test_all_five_exact_archives_are_staged_without_rebuilding(self):
        result = self.admit()
        self.assertEqual(result["status"], "PASS")
        self.assertFalse(result["qualification"])
        self.assertEqual(len(result["targets"]), 5)
        for target, (build, _, _) in self.paths.items():
            name = release.archive_name(self.version, target)
            self.assertEqual((self.output / name).read_bytes(), (build / name).read_bytes())
        digest = sha256(self.output / "archive-evidence.json")
        self.assertEqual(release.check_payload(self.output, self.version, self.revision, digest), result)

    def test_cross_build_without_native_execution_is_not_a_pass(self):
        target = "aarch64-unknown-linux-gnu"
        build_dir, execution, _ = self.paths[target]
        path = build_dir / "build.json"
        build = json.loads(path.read_text())
        build["command"][0] = "cross"
        build["compiler"] = build["compiler"].replace(target, "x86_64-unknown-linux-gnu")
        write_json(path, build)
        (execution / "package.json").unlink()
        with self.assertRaisesRegex(ValueError, "one archive execution"):
            self.admit()
        self.assertFalse(self.output.exists())

    def test_other_version_source_features_and_dirty_execution_are_rejected(self):
        _, execution, _ = self.paths["aarch64-apple-darwin"]
        path = execution / "package.json"
        original = json.loads(path.read_text())
        for field, value in (("version", "0.8.3-beta"), ("features", "telemetry"),
                             ("profile", "dev"),
                             ("test_source_revision", "f" * 40), ("test_worktree_dirty", True),
                             ("status", "BLOCKED"), ("suite_receipt_sha256", "0" * 64)):
            changed = copy.deepcopy(original)
            changed[field] = value
            write_json(path, changed)
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.admit()
            self.assertFalse(self.output.exists())

    def test_changed_bytes_or_prepared_dependency_inputs_cannot_be_published(self):
        build_dir, _, _ = self.paths["aarch64-apple-darwin"]
        path = build_dir / "build.json"
        original = json.loads(path.read_text())
        changed = copy.deepcopy(original)
        changed["inputs"]["stamped"]["Cargo.lock"] = "e" * 64
        write_json(path, changed)
        with self.assertRaisesRegex(ValueError, "Build inputs"):
            self.admit()
        write_json(path, original)
        (build_dir / original["archive"]["name"]).write_bytes(b"replacement")
        with self.assertRaisesRegex(ValueError, "archive bytes changed"):
            self.admit()
        self.assertFalse(self.output.exists())

    def test_mutated_junit_is_rechecked_instead_of_trusting_a_pass_label(self):
        _, _, attempt = self.paths["aarch64-apple-darwin"]
        (attempt / "cli/junit.xml").write_text("not the admitted result")
        with self.assertRaisesRegex(ValueError, "Changed test evidence"):
            self.admit()
        self.assertFalse(self.output.exists())

    def test_downloaded_payload_is_bound_to_the_gate_digest_and_exact_file_set(self):
        self.admit()
        manifest = self.output / "archive-evidence.json"
        digest = sha256(manifest)
        manifest.write_text(manifest.read_text() + " ")
        with self.assertRaisesRegex(ValueError, "gate output"):
            release.check_payload(self.output, self.version, self.revision, digest)
        # This branch uses the new digest only to reach the independent file-set
        # check. Production takes its digest from the gate job's immutable output.
        (self.output / "unreviewed.txt").write_text("extra")
        with self.assertRaisesRegex(ValueError, "unexpected files"):
            release.check_payload(self.output, self.version, self.revision, sha256(manifest))

    def test_workflow_requires_five_native_execution_hosts_and_never_rebuilds_for_publication(self):
        workflow = (Path(__file__).resolve().parents[2] / ".github/workflows/release.yml").read_text()
        build, rest = workflow.split("  test-archives:\n", 1)
        tests, rest = rest.split("  archive-gate:\n", 1)
        gate, publication = rest.split("  release:\n", 1)
        for section in (build, tests):
            targets = re.findall(r"^          - target: (\S+)$", section, re.M)
            self.assertEqual(set(targets), set(release.TARGETS))
            self.assertEqual(len(targets), 5)
        self.assertIn("os: ubuntu-24.04-arm", tests)
        self.assertIn("needs: [build, test-archives]", gate)
        self.assertIn("needs: [archive-gate, candidate-gate]", publication)
        self.assertIn("needs: [archive-gate, consumer, auxiliary]", gate)
        self.assertIn("os: [ubuntu-24.04, macos-26, windows-latest]", gate)
        self.assertIn("scripts/check-auxiliary-build.py", gate)
        self.assertIn('gate "${auxiliary_args[@]}"', gate)
        self.assertIn("scripts/qualify-release.py gate", gate)
        self.assertIn("needs.candidate-gate.outputs.qualification_sha256", publication)
        self.assertLess(publication.index("scripts/qualify-release.py verify"), publication.index("softprops/action-gh-release"))
        self.assertIn("if: github.event_name == 'push'", publication)
        self.assertNotIn("cargo build", publication)
        self.assertLess(publication.index("verify-payload"), publication.index("softprops/action-gh-release"))
        self.assertIn("needs.archive-gate.outputs.evidence_sha256", publication)


if __name__ == "__main__":
    unittest.main()
