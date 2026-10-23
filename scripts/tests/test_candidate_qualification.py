"""Synthetic qualification policy and real command refusal; no consumer pass claim."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import test_release_gate as archive_fixtures
from verification.evidence import sha256, write_json
from verification.qualification import (NATIVE, PORTABLE, check_qualification,
                                        evaluate_requirements)
from verification.release import TARGETS
from verification.requirements import case_results

REVISION = "a" * 40
SCRIPTS = Path(__file__).resolve().parents[1]


class CandidatePolicy(unittest.TestCase):
    def setUp(self):
        platforms = {t: "windows" if "windows" in t else "macos" if "darwin" in t else "linux" for t in TARGETS}
        self.classification = {"targets": platforms, "tests": [dict(id="fixture::command", boundary="X",
            platforms=["linux", "macos", "windows"], test_features=["all"])]}
        self.document = {"schema": 1, "requirements": []}
        for identity in sorted(NATIVE | PORTABLE):
            self.document["requirements"].append(dict(id=identity, title="Synthetic gate fixture",
                scope="stable" if identity in NATIVE else "v0.9-new", risk="critical", evidence_tiers=["X"],
                case_bindings_status="complete" if identity in NATIVE else "pending",
                cases=[dict(id=identity + ".fixture", boundary="X", test_ids=["fixture::command"],
                            required_platforms=["linux", "macos", "windows"],
                            required_product_features=["default"])] if identity in NATIVE else []))
        self.evidence = [dict(source_revision=REVISION, receipt_sha256=str(index) * 64,
            artifact_sha256=str(index + 5) * 64, target=target, features="default", test_features="all",
            status="PASS", tests=[dict(id="fixture::command", status="PASS")])
            for index, target in enumerate(TARGETS)]
        self.package_proofs = {r["receipt_sha256"] for r in self.evidence}

    def evaluate(self, version="0.8.4-beta"):
        return evaluate_requirements(self.document, self.classification, self.evidence,
                                     REVISION, version, self.package_proofs)

    def test_all_applicable_observations_required_and_future_portable_scope_retained(self):
        result = self.evaluate()
        self.assertTrue(result["qualification"], "synthetic complete policy fixture only")
        self.assertEqual(result["applicable_requirements"], sorted(NATIVE))
        self.assertEqual(len(result["acceptance"]["requirements"]), 109)
        result = self.evaluate("0.9.0-beta")
        self.assertEqual(result["status"], "BLOCKED")
        self.assertFalse(result["qualification"])
        self.assertEqual(result["applicable_requirements"], sorted(NATIVE | PORTABLE))

    def test_doctest_graphs_and_failures_participate_in_candidate_qualification(self):
        from test_doctest_evidence import RustdocAcceptance
        fixture = RustdocAcceptance()
        fixture.setUp()
        for record in fixture.records:
            record["receipt_sha256"] = "d" + record["receipt_sha256"][1:]
        row = next(row for row in self.document["requirements"] if row["id"] == "UX-086")
        row.update(evidence_tiers=["U"], cases=[fixture.case])
        def evaluate(records):
            return evaluate_requirements(self.document, self.classification, self.evidence,
                REVISION, "0.8.4-beta", self.package_proofs, doctest_evidence=records)
        self.assertEqual(evaluate([])["status"], "BLOCKED")
        self.assertTrue(evaluate(fixture.records)["qualification"], "synthetic gate mechanics only")
        self.assertFalse(evaluate(fixture.records[:-1])["qualification"])
        failed = dict(fixture.records[0], status="FAIL", tests=[], receipt_sha256="f" * 64)
        self.assertEqual(evaluate([*fixture.records, failed])["status"], "FAIL")

    def test_partial_mapping_and_missing_consumer_boundary_block_even_with_passing_cases(self):
        row = next(r for r in self.document["requirements"] if r["id"] == "UX-084")
        row["evidence_tiers"].append("C")
        result = self.evaluate()
        self.assertFalse(result["qualification"])
        self.assertIn("Missing required evidence boundaries: C", result["blockers"][0]["reasons"])
        row["evidence_tiers"] = ["X"]
        row["case_bindings_status"] = "partial"
        self.assertEqual(self.evaluate()["status"], "BLOCKED")

    def test_failed_or_unstarted_execution_cannot_qualify_by_other_passing_attempts(self):
        for status, overall in (("FAIL", "FAIL"), ("NOT_RUN", "BLOCKED"), ("BLOCKED", "BLOCKED")):
            self.evidence[0]["status"] = status
            self.evidence[0]["tests"][0]["status"] = status
            with self.subTest(status=status):
                result = self.evaluate()
                self.assertFalse(result["qualification"])
                self.assertEqual(result["status"], overall)
        self.evidence[0]["tests"][0]["status"] = "PASS"
        self.evidence[0]["status"] = "FAIL"
        self.assertEqual(self.evaluate()["status"], "FAIL", "runner failure cannot disappear behind passing rows")

    def test_native_requirements_cannot_be_removed_or_reclassified_as_future_work(self):
        removed = self.document["requirements"].pop()
        with self.assertRaisesRegex(ValueError, "retain all"):
            self.evaluate()
        self.document["requirements"].append(removed)
        row = next(r for r in self.document["requirements"] if r["id"] == "UX-001")
        row["scope"] = "v0.9-new"
        with self.assertRaisesRegex(ValueError, "applicability changed"):
            self.evaluate()

    def test_package_case_needs_proof_for_each_architecture_not_just_each_os(self):
        row = next(r for r in self.document["requirements"] if r["id"] == "UX-085")
        row["evidence_tiers"] = ["P"]
        row["cases"][0].update(boundary="P", required_targets=list(TARGETS))
        self.assertTrue(self.evaluate()["qualification"])
        self.package_proofs.remove(self.evidence[1]["receipt_sha256"])
        result = self.evaluate()
        self.assertEqual(result["status"], "BLOCKED")
        case = next(r for r in result["acceptance"]["requirements"] if r["id"] == "UX-085")["cases"][0]
        self.assertEqual(len(case["variants"]), 5)
        self.assertEqual([v["target"] for v in case["variants"] if v["status"] == "NOT_RUN"], [self.evidence[1]["target"]])
        # A self-reported field in an ordinary suite cannot confer archive proof.
        self.evidence[1]["execution_context"] = "packaged"
        self.assertFalse(self.evaluate()["qualification"])
        row["cases"][0]["required_targets"].remove(self.evidence[1]["target"])
        self.assertFalse(self.evaluate()["qualification"], "dropping ARM64 cannot shrink the release matrix")
        ordinary = case_results(self.document, self.classification, self.evidence, REVISION)
        case = next(r for r in ordinary["requirements"] if r["id"] == "UX-085")["cases"][0]
        self.assertTrue(all(v["status"] == "NOT_RUN" for v in case["variants"]))

    def test_publication_binds_qualification_to_source_version_payload_and_gate_digest(self):
        record = self.evaluate()
        record.update(schema=1, kind="release-candidate-qualification", source_revision=REVISION,
                      version="0.8.4-beta", archive_evidence_sha256="b" * 64)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "candidate.json"
            write_json(path, record)
            digest = sha256(path)
            check_qualification(path, digest, REVISION, "0.8.4-beta", "b" * 64)
            for field, value in (("qualification", False), ("status", "BLOCKED"),
                                 ("source_revision", "c" * 40), ("version", "0.8.3-beta"),
                                 ("archive_evidence_sha256", "d" * 64), ("blockers", [{}]),
                                 ("waivers", [{"approved": True}]), ("applicable_requirements", [])):
                changed = copy.deepcopy(record)
                changed[field] = value
                write_json(path, changed)
                with self.subTest(field=field), self.assertRaises(ValueError):
                    check_qualification(path, sha256(path), REVISION, "0.8.4-beta", "b" * 64)
            with self.assertRaisesRegex(ValueError, "gate output"):
                check_qualification(path, digest, REVISION, "0.8.4-beta", "b" * 64)


class CandidateCommand(unittest.TestCase):
    git = archive_fixtures.ReleaseArchiveGate.git
    setUp = archive_fixtures.ReleaseArchiveGate.setUp
    admit = archive_fixtures.ReleaseArchiveGate.admit

    def test_command_admits_doctest_graphs_but_rejects_failed_attempts(self):
        from test_doctest_evidence import fixture as doctest_fixture
        self.admit()
        output = self.root / ".git/doctest-candidate.json"
        command = [sys.executable, str(SCRIPTS / "qualify-release.py"), "gate",
                   "--payload", str(self.output), "--archive-evidence-sha256", sha256(self.output / "archive-evidence.json"),
                   "--revision", self.revision, "--version", self.version, "--output", str(output)]
        receipts = []
        for target in ("x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-pc-windows-msvc"):
            for graph in ("minimal", "all"):
                directory = self.root / ".git/doctest-fixtures" / target / graph
                record = doctest_fixture(directory, target, graph, source=self.root)
                record["source_revision"] = self.revision
                path = directory / "receipt.json"
                write_json(path, record)
                receipts.append(path)
                command += ["--doctest", str(path)]
        process = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(process.returncode, 2, process.stderr)
        result = json.loads(output.read_text())
        self.assertEqual(len(result["doctest_evidence"]), 6)
        row = next(row for row in result["acceptance"]["requirements"] if row["id"] == "UX-087")
        self.assertEqual(row["status"], "NOT_RUN", "synthetic passing examples cannot complete fresh-user acceptance")
        self.assertTrue(all(case["status"] == "PASS" for case in row["cases"]))
        failed = json.loads(receipts[0].read_text())
        failed.update(status="FAIL", exit_code=101)
        write_json(receipts[0], failed)
        process = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(process.returncode, 1, process.stderr)
        self.assertEqual(json.loads(output.read_text())["status"], "FAIL")
        receipts[0].unlink()
        process = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(process.returncode, 1, process.stderr)
        self.assertIn("reason", json.loads(output.read_text()))

    def test_real_registry_and_archives_cannot_skip_unimplemented_consumer_obligations(self):
        self.admit()
        output = self.root / ".git/candidate.json"
        command = [sys.executable, str(SCRIPTS / "qualify-release.py"), "gate",
                   "--payload", str(self.output), "--archive-evidence-sha256", sha256(self.output / "archive-evidence.json"),
                   "--revision", self.revision, "--version", self.version, "--output", str(output)]
        process = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(process.returncode, 2, process.stderr)
        record = json.loads(output.read_text())
        self.assertFalse(record["qualification"])
        self.assertEqual(len(record["applicable_requirements"]), 88)
        self.assertEqual(len(record["blockers"]), 88)
        row = next(r for r in record["acceptance"]["requirements"] if r["id"] == "UX-085")
        self.assertEqual(row["status"], "NOT_RUN", "one package case is still a partial requirement")
        self.assertEqual(row["cases"][0]["status"], "PASS", "synthetic archive admission mechanics, not real execution")
        self.assertEqual(len(row["cases"][0]["variants"]), 5)
        self.assertIn("UX-084", output.with_suffix(".md").read_text())
        verification = subprocess.run([sys.executable, str(SCRIPTS / "qualify-release.py"), "verify",
            "--record", str(output), "--sha256", sha256(output),
            "--archive-evidence-sha256", sha256(self.output / "archive-evidence.json"),
            "--revision", self.revision, "--version", self.version],
            cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertNotEqual(verification.returncode, 0)
        self.assertIn("unqualified", verification.stderr)
        # A wrong digest is a rejected admission, not a missing consumer result.
        command[command.index("--archive-evidence-sha256") + 1] = "0" * 64
        process = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(process.returncode, 1, process.stderr)
        self.assertEqual(json.loads(output.read_text())["status"], "FAIL")

    def test_parser_evidence_joins_without_qualifying_missing_consumer_journeys(self):
        from consumer_fixture import prism_record
        self.admit()
        parser = self.root / ".git/prism.json"
        data = prism_record(self.revision, self.root)
        write_json(parser, data)
        output = self.root / ".git/candidate.json"
        command = [sys.executable, str(SCRIPTS / "qualify-release.py"), "gate",
                   "--payload", str(self.output), "--archive-evidence-sha256", sha256(self.output / "archive-evidence.json"),
                   "--consumer", str(parser), "--revision", self.revision, "--version", self.version, "--output", str(output)]
        process = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(process.returncode, 2, process.stderr)
        result = json.loads(output.read_text())
        row = next(r for r in result["acceptance"]["requirements"] if r["id"] == "UX-053")
        self.assertEqual(row["status"], "NOT_RUN")
        self.assertEqual(row["cases"][0]["status"], "PASS")
        self.assertFalse(result["qualification"])
        self.assertEqual(result["consumer_evidence"][0]["receipt_sha256"], sha256(parser))
        data["fixtures"][0]["consumer"]["wrapper"] = "wrong wrapper"
        for status in ("PASS", "FAIL"):
            data["status"] = status
            write_json(parser, data)
            process = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
            self.assertEqual(process.returncode, 1, process.stderr)
            result = json.loads(output.read_text())
            self.assertEqual(result["status"], "FAIL")
            self.assertFalse(result["qualification"])
            if status == "FAIL":
                self.assertEqual(result["consumer_evidence"][0]["failed_fixtures"], ["supported-1.2"])
            else:
                self.assertIn("status disagrees", result["reason"])

    def test_python_observations_are_independent_and_cannot_qualify_missing_products(self):
        from test_python_evidence import fixture
        self.admit()
        output = self.root / ".git/python"
        classification = json.loads((self.root / "tests/verification/python-bindings.json").read_text())
        data = fixture(self.root, output, classification)
        data["source_revision"] = self.revision
        path = output / "receipt.json"
        write_json(path, data)
        candidate = self.root / ".git/candidate.json"
        command = [sys.executable, str(SCRIPTS / "qualify-release.py"), "gate",
                   "--payload", str(self.output), "--archive-evidence-sha256", sha256(self.output / "archive-evidence.json"),
                   "--tooling", str(path), "--revision", self.revision, "--version", self.version, "--output", str(candidate)]
        process = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(process.returncode, 2, process.stderr)
        result = json.loads(candidate.read_text())
        self.assertFalse(result["qualification"])
        tooling = result["tooling_evidence"][0]
        self.assertEqual(tooling["features"], "independent")
        self.assertNotIn("artifact_sha256", tooling)
        case = next(c for r in result["acceptance"]["requirements"] for c in r["cases"] if c["id"] == "UX-088.python-lifecycle")
        self.assertEqual(case["status"], "PASS")
        data["features"] = "telemetry"
        write_json(path, data)
        process = subprocess.run(command, cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(process.returncode, 1, process.stderr)
        self.assertEqual(json.loads(candidate.read_text())["status"], "FAIL")

    def test_supplemental_suite_cannot_substitute_another_product_or_unclean_source(self):
        self.admit()
        from verification.qualification import qualify
        _, _, attempt = self.paths["aarch64-apple-darwin"]
        path = attempt / "receipt.json"
        original = json.loads(path.read_text())
        for field, value in (("sha256", "d" * 64), ("features", "telemetry"),
                             ("profile", "dev"), ("version", "0.8.3-beta")):
            modified = copy.deepcopy(original)
            modified["artifact"][field] = value
            write_json(path, modified)
            with self.subTest(field=field), self.assertRaises(ValueError):
                qualify(self.root, self.output, sha256(self.output / "archive-evidence.json"),
                        self.revision, self.version, [path])
        write_json(path, original)
        (self.root / "Cargo.toml").write_text((self.root / "Cargo.toml").read_text() + "\n")
        with self.assertRaisesRegex(ValueError, "clean source"):
            qualify(self.root, self.output, sha256(self.output / "archive-evidence.json"),
                    self.revision, self.version)


if __name__ == "__main__":
    unittest.main()
