"""Coverage wiring and failure preservation, not instrumented-product acceptance."""
from contextlib import redirect_stderr
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shlex
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("coverage_runner", SCRIPTS / "run-coverage.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class CoverageRunner(unittest.TestCase):
    def settings(self):
        values = {"CARGO_LLVM_COV": "1", "RUSTC_WRAPPER": "/tool path/cargo-llvm-cov",
                  "__CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS": "-C\x1finstrument-coverage\x1f--cfg=coverage"}
        return "\n".join("export " + key + "=" + shlex.quote(value) for key, value in values.items())

    def test_quoted_environment_preserves_instrumentation_without_shell_evaluation(self):
        settings = self.settings() + "\nexport LLVM_PROFILE_FILE=" + shlex.quote("/path ' Ω; literal/coverage-%p.profraw")
        environment = runner.instrumented_environment(settings, {"PATH": "fixture tools"})
        self.assertEqual(environment["PATH"], "fixture tools")
        self.assertEqual(environment["LLVM_PROFILE_FILE"], "/path ' Ω; literal/coverage-%p.profraw")
        self.assertEqual(environment["__CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS"],
                         "-C\x1finstrument-coverage\x1f--cfg=coverage")

    def test_unexpected_or_missing_instrumentation_assignments_fail(self):
        for settings in ["", "export FLAG=value", self.settings() + "\nexecute something",
                         self.settings() + "\nexport BAD-NAME=value"]:
            with self.subTest(settings=settings), self.assertRaises(ValueError):
                runner.instrumented_environment(settings, {})

    def test_inventory_binds_test_process_and_report_cannot_erase_test_failure(self):
        inventory = json.dumps({"test-count": 1, "rust-suites": {"compiled-library": {"kind": "lib"}}})
        for test_status, report_status, expected in [(100, 0, 100), (0, 9, 9), (3, 9, 3)]:
            with self.subTest(tests=test_status, report=report_status), tempfile.TemporaryDirectory() as directory:
                seen = []

                def worker(command, **kwargs):
                    seen.append((command, kwargs))
                    name = kwargs["log"].stem
                    if name == "environment":
                        return 0, self.settings(), False
                    if name == "discovery":
                        return 0, inventory, False
                    return {"tests": test_status, "report": report_status}.get(name, 0), "", False

                with patch.object(runner, "run", side_effect=worker):
                    self.assertEqual(runner.coverage(Path(directory), {"PATH": "fixture tools"}), expected)
                tests = next(row for row in seen if row[1]["log"].stem == "tests")
                environment = tests[1]["env"]
                selected = Path(environment["EMPACK_TEST_INVENTORY"])
                self.assertTrue(selected.is_absolute())
                self.assertEqual(selected.read_text(), inventory)
                self.assertEqual(environment["EMPACK_TEST_INVENTORY_SHA256"],
                                 hashlib.sha256(selected.read_bytes()).hexdigest())
                self.assertEqual(tests[0][-3:], ["--retries", "0", "--no-fail-fast"])
                self.assertEqual(seen[-1][1]["log"].stem, "report")
                self.assertNotIn("EMPACK_TEST_INVENTORY", seen[-1][1]["env"])

    def test_failed_discovery_does_not_execute_tests_or_report(self):
        seen = []
        with tempfile.TemporaryDirectory() as directory:
            def worker(command, **kwargs):
                name = kwargs["log"].stem
                seen.append(name)
                return (1, "", False) if name == "discovery" else (
                    0, self.settings() if name == "environment" else "", False)

            with patch.object(runner, "run", side_effect=worker), self.assertRaisesRegex(RuntimeError, "discovery failed"):
                runner.coverage(Path(directory), {})
        self.assertNotIn("tests", seen)
        self.assertNotIn("report", seen)

    def test_failed_test_diagnostics_survive_a_successful_report(self):
        with tempfile.TemporaryDirectory() as directory, redirect_stderr(io.StringIO()) as console:
            def worker(command, **options):
                name = options["log"].stem
                if name == "environment":
                    return 0, self.settings(), False
                if name == "discovery":
                    return 0, json.dumps({"test-count":1,"rust-suites":{"fixture":{}}}), False
                if name == "tests":
                    options["log"].write_text("omitted early context\n" + "context\n" * 60 + "FAIL fixture::selected_case <redacted>\n")
                    return 100, "stdout without the failure", False
                return 0, "", False
            with patch.object(runner, "run", side_effect=worker):
                self.assertEqual(runner.coverage(Path(directory), {}), 100)
            self.assertIn("FAIL fixture::selected_case <redacted>", console.getvalue())
            self.assertNotIn("omitted early context", console.getvalue())
            self.assertLessEqual(len(console.getvalue().splitlines()), 42)

    def test_workflow_retains_coverage_diagnostics_after_failure(self):
        workflow = (SCRIPTS.parent / ".github/workflows/pr-ci.yml").read_text()
        block = workflow.split("  coverage:\n", 1)[1].split("  gate:\n", 1)[0]
        step = block.split("      - name: Retain coverage diagnostics\n", 1)[1].split("\n      - ", 1)[0]
        self.assertIn("if: always()", step)
        self.assertIn("uses: actions/upload-artifact@v7", step)
        self.assertIn("name: verification-coverage", step)
        self.assertIn("target/verification/coverage/", step)
        self.assertIn("lcov.info", step)


if __name__ == "__main__":
    unittest.main()
