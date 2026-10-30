"""Synthetic admission and dispatch checks; no product or live suite is executed."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import test_auxiliary_build as auxiliary_fixtures
import test_ci_verification as ci
from verification.candidate_execution import GRAPHS, PHASES, inspect_execution, inspect_runs
from verification.evidence import sha256, write_json
from verification.release import check_payload
from verification.auxiliary import inspect_auxiliary
from verification.suites import CLI, SUITES

spec = importlib.util.spec_from_file_location("candidate_driver", ci.SCRIPTS / "check-candidate-execution.py")
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


class CandidateExecution(unittest.TestCase):
    setUp = auxiliary_fixtures.AuxiliaryBuild.setUp
    git = auxiliary_fixtures.AuxiliaryBuild.git
    admit = auxiliary_fixtures.AuxiliaryBuild.admit
    auxiliary_fixture = auxiliary_fixtures.AuxiliaryBuild.fixture

    def fixture(self):
        self.admit()
        self.archive_digest = sha256(self.output / "archive-evidence.json")
        self.archives = check_payload(self.output, self.version, self.revision, self.archive_digest)
        auxiliary, _, _ = self.auxiliary_fixture()
        self.auxiliary_path = auxiliary
        self.auxiliary = [inspect_auxiliary(auxiliary, self.root, self.revision, self.version)]
        target = self.auxiliary[0]["suite"]["target"]
        default = next(row for row in self.archives["targets"] if row["suite"]["target"] == target)
        artifacts = dict(default=default["executable"]["sha256"], telemetry=self.auxiliary[0]["executable"]["sha256"])
        directory = self.root / ".git/execution"
        for features, graph in GRAPHS.items():
            for phase, selection in PHASES.items():
                with patch.object(ci, "REVISION", self.revision):
                    attempt, receipt = ci.fixture_attempt(directory / features / phase, target, features, selection, graph)
                receipt["artifact"].update(version=self.version, profile="release", sha256=artifacts[features])
                receipt["rustc"] = self.auxiliary[0]["compiler"]
                write_json(attempt / "receipt.json", receipt)
        self.path = directory / "execution.json"
        self.record = dict(schema=1, kind="candidate-native-execution", qualification=False,
            source_revision=self.revision, version=self.version, target=target,
            created_at="2026-10-30T00:00:00+00:00", archive_evidence_sha256=self.archive_digest,
            auxiliary_build_sha256=self.auxiliary[0]["build_sha256"], artifacts=artifacts, status="PASS")
        self.record["compiler"] = self.auxiliary[0]["compiler"]
        self.record["runs"], _ = inspect_runs(directory, target, artifacts, self.revision, self.version,
                                              self.record["compiler"])
        write_json(self.path, self.record)

    def inspect(self):
        return inspect_execution(self.path, self.archives, self.auxiliary,
                                 self.archive_digest, self.revision, self.version)

    def test_both_features_native_and_live_are_bound_without_becoming_package_evidence(self):
        self.fixture()
        result = self.inspect()
        self.assertEqual(result["status"], "PASS", "synthetic admission only")
        self.assertEqual(len(result["suites"]), 4)
        native = [row for row in result["suites"] if row["suites"] == PHASES["native"]]
        self.assertEqual({row["features"] for row in native}, {"default", "telemetry"})
        for row in native:
            self.assertTrue(row["tests"])
            binaries = {test["id"].split("::")[1] for test in row["tests"]
                        if test["suite"] == "candidate-native"}
            self.assertFalse(binaries & set(CLI), "archive and auxiliary CLI observations are reused")
        for row in result["suites"]:
            self.assertNotIn("package", row)
        output = self.root / ".git/qualification.json"
        command = [sys.executable, str(ci.SCRIPTS / "qualify-release.py"), "gate", "--payload", str(self.output),
                   "--archive-evidence-sha256", self.archive_digest, "--auxiliary", str(self.auxiliary_path),
                   "--execution", str(self.path), "--revision", self.revision, "--version", self.version,
                   "--output", str(output)]
        completed = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(completed.returncode, 2, completed.stderr)
        qualified = json.loads(output.read_text())
        self.assertFalse(qualified["qualification"], "synthetic native/live data cannot complete actual consumers")
        self.assertEqual(len(qualified["candidate_executions"]), 1)
        self.assertEqual(len(qualified["suite_evidence"]), 10)

    def test_changed_payload_source_binary_selection_or_missing_attempt_cannot_qualify(self):
        self.fixture()
        for key, changed in (("source_revision", "f" * 40), ("archive_evidence_sha256", "0" * 64),
                             ("artifacts", dict(self.record["artifacts"], telemetry="0" * 64)),
                             ("runs", self.record["runs"][:-1]), ("qualification", True), ("compiler", "other"),
                             ("reason", "source changed")):
            write_json(self.path, dict(self.record, **{key: changed}))
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.inspect()
        write_json(self.path, self.record)
        for value in (None, [], "record"):
            write_json(self.path, value)
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "record object"):
                self.inspect()
        write_json(self.path, self.record)
        receipt = self.path.parent / "default/native/one-attempt/receipt.json"
        raw = receipt.read_bytes()
        write_json(receipt, [])
        with self.assertRaisesRegex(ValueError, "receipt object"):
            self.inspect()
        receipt.write_bytes(raw)
        receipt.unlink()
        with self.assertRaisesRegex(ValueError, "one contained attempt"):
            self.inspect()
        receipt.write_bytes(raw)
        duplicate = receipt.parent.parent / "second-attempt"
        duplicate.mkdir()
        (duplicate / "receipt.json").write_bytes(raw)
        with self.assertRaisesRegex(ValueError, "one contained attempt"):
            self.inspect()

    def test_blocked_live_evidence_retains_other_phases_and_cannot_be_reported_as_pass(self):
        self.fixture()
        path = self.path.parent / "default/live/one-attempt/receipt.json"
        receipt = json.loads(path.read_text())
        receipt["suites"] = [dict(suite=row["suite"], status="BLOCKED", reason="missing fixture credential")
                             if row["suite"] == "live-curseforge" else row for row in receipt["suites"]]
        receipt["status"] = "BLOCKED"
        write_json(path, receipt)
        self.record["runs"], _ = inspect_runs(self.path.parent, self.record["target"], self.record["artifacts"],
                                              self.revision, self.version, self.record["compiler"])
        write_json(self.path, self.record)
        with self.assertRaisesRegex(ValueError, "summary differs"):
            self.inspect()
        self.record["status"] = "BLOCKED"
        write_json(self.path, self.record)
        result = self.inspect()
        self.assertEqual(result["status"], "BLOCKED")
        self.assertEqual([row["status"] for row in result["suites"]], ["PASS", "BLOCKED", "PASS", "PASS"])


class CandidateDispatch(unittest.TestCase):
    def test_fixed_dispatch_continues_after_blocked_phase_without_rebuilding_products(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binaries = {name: root / name for name in GRAPHS}
            for name, path in binaries.items():
                path.write_bytes(("inert " + name).encode())
            artifacts = {name: sha256(path) for name, path in binaries.items()}
            calls = []
            def observed(command):
                calls.append(command)
                output = Path(command[command.index("--output") + 1]) / "one-attempt"
                output.mkdir(parents=True)
                write_json(output / "receipt.json", dict(status="BLOCKED"))
                return 2
            with patch.object(driver.verify, "main", side_effect=observed):
                driver.execute(root / "runs", binaries, artifacts, "aarch64-apple-darwin", "0.8.4-beta")
            self.assertEqual(len(calls), 4)
            for command, (features, phase) in zip(calls, ((f, p) for f in GRAPHS for p in PHASES), strict=True):
                self.assertEqual(command[:len(PHASES[phase])], PHASES[phase])
                self.assertEqual(command[command.index("--binary") + 1], str(binaries[features]))
                self.assertEqual(command[command.index("--test-features") + 1], GRAPHS[features])
                self.assertIn("live-runtime", PHASES["live"])
                self.assertNotIn("runtime-current", PHASES["live"])

    def test_fixed_exclusion_rejects_unexpected_skipped_or_discovered_binaries(self):
        from verification.evidence import selected_tests
        from verification.classification import compare_rust_inventory
        native = SUITES["candidate-native"]
        inventory = {"test-count": 1, "rust-suites": {
            "host": {"status": "listed", "binary-name": "empack_lib", "testcases": {
                "native": {"ignored": False, "filter-match": {"status": "matches"}}}},
            "cli": {"status": "skipped", "binary-name": "cli_arguments", "testcases": {}}}}
        selected, _ = selected_tests(inventory, False, excluded_binaries=native.excluded_binaries)
        self.assertEqual(selected, ["host::native"])
        inventory["rust-build-meta"] = {"platforms": {"host": {"platform": {"triple": "native"}}, "targets": []}}
        classification = dict(schema=3, selection="workspace-feature-graphs", targets={"native": "linux"}, tests=[
            dict(id=identity, binary=binary, binary_name=name, boundary="N", api="TEST", platforms=["linux"],
                 test_features=["minimal", "all"], ignored=False)
            for identity, binary, name in (("host::native", "host", "empack_lib"),
                                           ("cli::version", "cli", "cli_arguments"))])
        discovered = compare_rust_inventory(inventory, classification, {"TEST"},
                                             excluded_binaries=("cli_arguments",))
        self.assertEqual(set(discovered), {"host::native"})
        with self.assertRaisesRegex(ValueError, "not listed"):
            compare_rust_inventory(inventory, classification, {"TEST"})
        with self.assertRaisesRegex(ValueError, "both required and excluded"):
            compare_rust_inventory(inventory, classification, {"TEST"}, ("cli_arguments",),
                                   excluded_binaries=("cli_arguments",))
        changed = copy.deepcopy(inventory)
        changed["rust-suites"]["cli"]["binary-name"] = "unexpected"
        with self.assertRaisesRegex(ValueError, "Unlisted"):
            selected_tests(changed, False, excluded_binaries=native.excluded_binaries)
        changed = copy.deepcopy(inventory)
        changed["rust-suites"]["cli"].update(status="listed")
        with self.assertRaisesRegex(ValueError, "Excluded"):
            selected_tests(changed, False, excluded_binaries=native.excluded_binaries)


class CandidateCapture(unittest.TestCase):
    def test_retained_binary_capture_preserves_input_and_sets_only_owned_output_attributes(self):
        from verification.package_archive import capture_binary
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source, destination = root / "downloaded", root / "owned"
            source.write_bytes(b"inert native executable fixture")
            if os.name != "nt":
                source.chmod(0o644)
            original_mode = stat.S_IMODE(source.stat().st_mode)
            digest = sha256(source)
            capture_binary(source, destination, digest)
            self.assertEqual(sha256(destination), digest)
            self.assertEqual(stat.S_IMODE(source.stat().st_mode), original_mode)
            if os.name != "nt":
                self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o755)
            with self.assertRaises(FileExistsError):
                capture_binary(source, destination, digest)
            with self.assertRaisesRegex(ValueError, "expected SHA-256"):
                capture_binary(source, root / "wrong", "0" * 64)
            with patch("verification.package_archive.MAX_BINARY", 2), self.assertRaisesRegex(ValueError, "byte limit"):
                capture_binary(source, root / "bounded", digest)
            self.assertEqual(sha256(source), digest)
