"""Synthetic lifecycle records test admission; they are not product executions."""
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from verification.evidence import sha256, write_json
from verification.python_tests import (BINDINGS, PRODUCER_INPUTS, ObservedResult, compare_inventory,
                                       inputs, inspect_python_evidence, outcomes)

REVISION = "a" * 40


def fixture(root, output, classification=None):
    """Small synthetic evidence set; never executed or used as real qualification."""
    output.mkdir(parents=True, exist_ok=True)
    if classification is None:
        (root / BINDINGS).parent.mkdir(parents=True)
        classification = {"schema": 1, "tests": [{"id": "python::fixture.Case.test_pass", "boundary": "U"}]}
        (root / "scripts").mkdir()
        (root / "scripts/fixture.py").write_text("# synthetic source\n")
        for name in PRODUCER_INPUTS:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("# synthetic producer identity; not executable evidence\n")
        write_json(root / BINDINGS, classification)
        write_json(root / "tests/verification/requirements.json", {"synthetic": True})
    inventory = {"schema": 1, "source": "unittest-loader", "tests": [
        {"id": row["id"].removeprefix("python::"), "ignored": False} for row in classification["tests"]]}
    events = [event for row in inventory["tests"] for event in (
        dict(id=row["id"], event="start"), dict(id=row["id"], event="result", status="PASS"), dict(id=row["id"], event="stop"))]
    write_json(output / "inventory.json", inventory)
    write_json(output / "events.json", events)
    tests, status = outcomes(inventory, events)
    record = dict(schema=1, kind="python-suite-run", source_revision=REVISION, worktree_dirty=False,
                  features="independent", platform="linux", architecture="x86_64", python_version="3.14.0",
                  interpreter_sha256="c" * 64, created_at="2026-10-23T00:00:00+00:00", inputs=inputs(root),
                  inventory_sha256=sha256(output / "inventory.json"), events_sha256=sha256(output / "events.json"),
                  classification_sha256=sha256(root / BINDINGS), tests=tests, status=status, exit_code=0, timed_out=False)
    write_json(output / "receipt.json", record)
    return record


class PythonLifecycle(unittest.TestCase):
    def test_actual_callbacks_keep_subtest_failure_skip_and_expected_failure(self):
        class Cases(unittest.TestCase):
            def test_pass(self):
                pass
            def test_subtest(self):
                for value in [False, True]:
                    with self.subTest(value=value):
                        self.assertTrue(value)
            @unittest.skip("not executed")
            def test_skip(self):
                self.fail("must not execute")
            @unittest.expectedFailure
            def test_expected(self):
                self.fail("still a failed assertion")
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(Cases)
        inventory = {"tests": [{"id": test.id(), "ignored": test._testMethodName == "test_skip"} for test in suite]}
        result = unittest.TextTestRunner(stream=io.StringIO(), resultclass=ObservedResult).run(suite)
        tests, status = outcomes(inventory, result.events)
        self.assertEqual(status, "FAIL")
        self.assertEqual({row["id"].rsplit(".", 1)[1]: row["status"] for row in tests},
                         dict(test_pass="PASS", test_subtest="FAIL", test_skip="NOT_RUN", test_expected="FAIL"))

    def test_class_setup_failure_cannot_become_empty_success(self):
        class Cases(unittest.TestCase):
            @classmethod
            def setUpClass(cls):
                raise RuntimeError("fixture unavailable")
            def test_unstarted(self):
                self.fail("must not execute")
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(Cases)
        inventory = {"tests": [{"id": test.id(), "ignored": False} for test in suite]}
        result = unittest.TextTestRunner(stream=io.StringIO(), resultclass=ObservedResult).run(suite)
        tests, status = outcomes(inventory, result.events)
        self.assertEqual(status, "FAIL")
        self.assertEqual(tests[0]["status"], "NOT_RUN")

    def test_duplicate_unstarted_and_incomplete_callbacks_do_not_pass(self):
        inventory = {"tests": [{"id": "case", "ignored": False}]}
        start, stop = dict(id="case", event="start"), dict(id="case", event="stop")
        passed = dict(id="case", event="result", status="PASS")
        self.assertEqual(outcomes(inventory, [start, passed])[1], "NOT_RUN")
        for events in ([passed], [start, stop], [start, passed, stop, start, passed, stop],
                       [dict(id="unknown", event="result", status="PASS")]):
            with self.subTest(events=events), self.assertRaises(ValueError):
                outcomes(inventory, events)
        failed = dict(id="case", event="subtest", status="FAIL")
        self.assertEqual(outcomes(inventory, [start, failed, passed, stop])[1], "FAIL")


class PythonAdmission(unittest.TestCase):
    def test_replays_observations_instead_of_claimed_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            root, output = Path(temp), Path(temp) / "attempt"
            record = fixture(root, output)
            self.assertEqual(inspect_python_evidence(output / "receipt.json", REVISION, root)["status"], "PASS")
            events = json.loads((output / "events.json").read_text())
            events[1]["status"] = "FAIL"
            write_json(output / "events.json", events)
            record["events_sha256"] = sha256(output / "events.json")
            write_json(output / "receipt.json", record)
            with self.assertRaisesRegex(ValueError, "observed lifecycle"):
                inspect_python_evidence(output / "receipt.json", REVISION, root)
            record.update(status="FAIL", exit_code=1)
            record["tests"][0]["status"] = "FAIL"
            write_json(output / "receipt.json", record)
            self.assertEqual(inspect_python_evidence(output / "receipt.json", REVISION, root)["status"], "FAIL")

    def test_identity_and_artifact_tampering_cannot_be_admitted(self):
        with tempfile.TemporaryDirectory() as temp:
            root, output = Path(temp), Path(temp) / "attempt"
            record = fixture(root, output)
            for change in [dict(worktree_dirty=True), dict(source_revision="b" * 40), dict(features="default"),
                           dict(python_version="2.7.0"), dict(inventory_sha256="d" * 64),
                           dict(events_sha256="d" * 64), dict(timed_out=True)]:
                with self.subTest(change=change):
                    write_json(output / "receipt.json", dict(record, **change))
                    with self.assertRaises(ValueError):
                        inspect_python_evidence(output / "receipt.json", REVISION, root)
            write_json(output / "receipt.json", record)
            (root / "scripts/fixture.py").write_text("# changed source\n")
            with self.assertRaisesRegex(ValueError, "Unrelated"):
                inspect_python_evidence(output / "receipt.json", REVISION, root)

    def test_classification_rejects_unknown_duplicate_or_consumer_identities(self):
        inventory = dict(schema=1, source="unittest-loader", tests=[dict(id="case", ignored=False)])
        good = dict(tests=[dict(id="python::case", boundary="U")])
        compare_inventory(inventory, good)
        for change in [dict(tests=[]), dict(tests=good["tests"] * 2),
                       dict(tests=[dict(id="python::case", boundary="C")]),
                       dict(tests=[dict(id="python::other", boundary="U")])]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                compare_inventory(inventory, change)


class PythonReport(unittest.TestCase):
    def test_executable_report_keeps_failed_then_passing_tooling_attempts(self):
        import subprocess
        from verification.python_tests import ROOT
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            classification = json.loads((ROOT / BINDINGS).read_text())
            fixture(ROOT, root / "pass", classification)
            failing = fixture(ROOT, root / "fail", classification)
            events = json.loads((root / "fail/events.json").read_text())
            events[1]["status"] = "FAIL"
            write_json(root / "fail/events.json", events)
            failing["events_sha256"] = sha256(root / "fail/events.json")
            failing["tests"][0]["status"] = "FAIL"
            failing.update(status="FAIL", exit_code=1)
            write_json(root / "fail/receipt.json", failing)
            command = [sys.executable, str(ROOT / "scripts/report-verification.py"),
                       "--receipt", str(root / "fail/receipt.json"), "--receipt", str(root / "pass/receipt.json"),
                       "--revision", REVISION, "--version", "0.0.0-dev", "--output", str(root / "report")]
            result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 1, result.stderr)
            report = json.loads((root / "report/attempts.json").read_text())
            self.assertEqual([row["status"] for row in report["attempts"]], ["FAIL", "PASS"])
            self.assertTrue(all(row["admission"] == "ADMITTED" for row in report["attempts"]))
            self.assertFalse(report["qualification"])
            self.assertFalse(report["suite_evidence"])
            text = (root / "report/attempts.md").read_text()
            self.assertIn("No empack artifact identity is established", text)
            self.assertIn("Python tooling observations", text)
