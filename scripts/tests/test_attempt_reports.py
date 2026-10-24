"""Synthetic evidence exercises admission/reporting, not product qualification."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

from test_ci_verification import REVISION, SCRIPTS, ci, fixture_attempt
from verification.evidence import sha256, write_json
from verification.receipts import check_suite_receipt, inspect_suite_receipt

TARGET = ci.TARGETS[0]


def fail_case(attempt, receipt, identity=None):
    row = receipt["suites"][0]
    identity = identity or row["selected"][0]
    path = attempt / row["suite"] / "junit.xml"
    tree = ET.parse(path)
    for case in tree.findall(".//testcase"):
        if f"{case.attrib['classname']}::{case.attrib['name']}" == identity:
            ET.SubElement(case, "failure", message="controlled fixture failure")
    tree.write(path, encoding="utf-8")
    row.update(status="FAIL", exit_code=100, junit_sha256=sha256(path))
    for test in row["tests"]:
        if test["id"] == identity:
            test["status"] = "FAIL"
    receipt["status"] = "FAIL"
    write_json(attempt / "receipt.json", receipt)
    return identity


class AttemptAdmission(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.attempt, self.receipt = fixture_attempt(self.root, TARGET, suites=["telemetry"])

    def inspect(self):
        write_json(self.attempt / "receipt.json", self.receipt)
        return inspect_suite_receipt(self.attempt / "receipt.json", ["telemetry"], TARGET, REVISION)

    def test_failed_junit_is_retained_but_never_accepted_by_strict_gate(self):
        identity = fail_case(self.attempt, self.receipt)
        result = self.inspect()
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(next(t["status"] for t in result["tests"] if t["id"] == identity), "FAIL")
        with self.assertRaisesRegex(ValueError, "Failed or unrelated"):
            check_suite_receipt(self.root, ["telemetry"], TARGET, REVISION)
        self.receipt["suites"][0]["tests"][0]["status"] = "PASS"
        with self.assertRaisesRegex(ValueError, "inconsistent"):
            self.inspect()

    def test_timeout_without_junit_cannot_supply_passing_tests(self):
        (self.attempt / "telemetry/junit.xml").unlink()
        row = self.receipt["suites"][0]
        row.pop("junit_sha256")
        row.update(status="FAIL", exit_code=-9, timed_out=True)
        for test in row["tests"]:
            test["status"] = "NOT_RUN"
        self.receipt["status"] = "FAIL"
        result = self.inspect()
        self.assertEqual(result["status"], "FAIL")
        self.assertTrue(all(test["status"] == "NOT_RUN" for test in result["tests"]))
        (self.attempt / "telemetry/junit.xml").write_text("unbound stale output")
        with self.assertRaisesRegex(ValueError, "Unbound"):
            self.inspect()

    def test_preflight_records_blocker_without_fabricating_an_executable(self):
        self.receipt.pop("artifact")
        self.receipt.update(status="BLOCKED", reason="Missing product prerequisite",
                            rustc=f"rustc fixture\nhost: {TARGET}",
                            requested_product_features="telemetry", requested_product_profile="dev",
                            suites=[dict(suite="telemetry", status="NOT_RUN", tests=[])])
        result = self.inspect()
        self.assertEqual(result["status"], "BLOCKED")
        self.assertEqual(result["tests"], [])
        self.assertIsNone(result["artifact_sha256"])
        for key, value in (("requested_product_features", "default"),
                           ("requested_product_profile", "release"), ("rustc", "host: other-target")):
            old = self.receipt[key]
            self.receipt[key] = value
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "preflight"):
                self.inspect()
            self.receipt[key] = old
        self.receipt["suites"][0]["tests"] = [dict(id="invented", status="PASS")]
        with self.assertRaisesRegex(ValueError, "Unexecuted"):
            self.inspect()

    def test_changed_inputs_or_evidence_cannot_reuse_earlier_pass_rows(self):
        for reason in ("Product artifact changed during verification", "Source files changed during verification"):
            self.receipt.update(status="FAIL", reason=reason)
            with self.subTest(reason=reason), self.assertRaisesRegex(ValueError, "stable execution inputs"):
                self.inspect()
        self.receipt.pop("reason")
        self.receipt["status"] = "PASS"
        (self.attempt / "telemetry/junit.xml").write_text("<broken>")
        with self.assertRaisesRegex(ValueError, "Changed test evidence"):
            self.inspect()
        self.receipt["suites"][0]["junit_sha256"] = sha256(self.attempt / "telemetry/junit.xml")
        with self.assertRaisesRegex(ValueError, "Malformed JUnit"):
            self.inspect()

    def test_unstarted_suites_do_not_hide_failed_or_blocked_suites(self):
        self.receipt.update(requested_suites=["telemetry", "deterministic"], suites=[
            dict(suite="telemetry", status="FAIL", reason="discovery failed", tests=[]),
            dict(suite="deterministic", status="NOT_RUN", tests=[])], status="FAIL")
        for state in ("FAIL", "BLOCKED"):
            self.receipt["status"] = state
            self.receipt["suites"][0]["status"] = state
            write_json(self.attempt / "receipt.json", self.receipt)
            result = inspect_suite_receipt(self.attempt / "receipt.json", ["telemetry", "deterministic"], TARGET, REVISION)
            self.assertEqual(result["status"], state)
            self.assertEqual(result["tests"], [])
            self.assertEqual(result["suite_outcomes"][0]["reason"], "discovery failed")


class AttemptReportCommand(unittest.TestCase):
    def run_report(self, root, paths):
        output = root / "report"
        command = [sys.executable, str(SCRIPTS / "report-verification.py"),
                   "--revision", REVISION, "--version", "0.8.4-beta", "--output", str(output)]
        for path in paths:
            command += ["--receipt", str(path)]
        process = subprocess.run(command, capture_output=True, text=True, timeout=30)
        self.assertTrue((output / "attempts.json").exists(), process.stderr)
        return process, json.loads((output / "attempts.json").read_text()), (output / "attempts.md").read_text()

    def test_failed_then_passing_attempt_retains_both_in_executable_report(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            failed, first = fixture_attempt(root / "failed", TARGET, suites=["deterministic"])
            passed, second = fixture_attempt(root / "passed", TARGET, suites=["deterministic"])
            selected = set(first["suites"][0]["selected"])
            requirement = next(row for row in json.loads(ci.REQUIREMENTS.read_text())["requirements"]
                               if any(set(case["test_ids"]) & selected for case in row["cases"]))
            identity = next(identity for case in requirement["cases"] for identity in case["test_ids"] if identity in selected)
            first["artifact"]["version"] = second["artifact"]["version"] = "0.8.4-beta"
            fail_case(failed, first, identity)
            write_json(passed / "receipt.json", second)
            paths = [failed / "receipt.json", passed / "receipt.json", failed / "receipt.json"]
            process, report, text = self.run_report(root, paths)
            self.assertEqual(process.returncode, 1, process.stderr)
            self.assertEqual(report["status"], "FAIL")
            self.assertFalse(report["qualification"])
            self.assertEqual([a["status"] for a in report["attempts"]], ["FAIL", "PASS"])
            joined = next(row for row in report["acceptance"]["requirements"] if row["id"] == requirement["id"])
            self.assertEqual(joined["status"], "FAIL")
            for path in paths:
                self.assertIn(sha256(path), text)
            self.assertIn("not release qualification", text)
            self.assertIn(identity, text)

    def test_blocked_and_rejected_inputs_remain_distinct_and_escaped(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            attempt, receipt = fixture_attempt(root / "blocked", TARGET, suites=["telemetry"])
            receipt.pop("artifact")
            receipt.update(status="BLOCKED", reason='<script>missing</script> | prerequisite',
                           rustc=f"host: {TARGET}", requested_product_features="telemetry",
                           requested_product_profile="dev", suites=[dict(suite="telemetry", status="NOT_RUN", tests=[])])
            write_json(attempt / "receipt.json", receipt)
            process, report, text = self.run_report(root, [attempt / "receipt.json"])
            self.assertEqual(process.returncode, 2, process.stderr)
            self.assertEqual(report["status"], "BLOCKED")
            self.assertNotIn("<script>", text)
            self.assertIn("&lt;script&gt;missing&lt;/script&gt; &#124; prerequisite", text)
            other = root / "rejected"
            other.mkdir()
            invalid = other / "receipt.json"
            invalid.write_text("[]")
            process, report, text = self.run_report(other, [invalid, other / "missing.json"])
            self.assertEqual(process.returncode, 1, process.stderr)
            self.assertEqual([a["admission"] for a in report["attempts"]], ["REJECTED", "REJECTED"])
            self.assertIsNone(report["attempts"][1]["receipt_sha256"])
            self.assertEqual(report["suite_evidence"], [])
            self.assertIn("No test outcomes", text)


if __name__ == "__main__":
    unittest.main()
