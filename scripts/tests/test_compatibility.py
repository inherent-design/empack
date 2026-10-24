"""Check dated selections and the nonqualifying scheduled runner boundary."""
from datetime import date, timedelta
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
from verification.compatibility import scheduled_suites, weekly_index
from verification.suites import SUITES

SPEC = importlib.util.spec_from_file_location("compatibility_runner", SCRIPTS / "check-live-compatibility.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class CompatibilitySelection(unittest.TestCase):
    def test_each_week_selects_one_slot_and_wraps_across_year_boundaries(self):
        start = date(2026, 12, 28)
        slots = [weekly_index(start + timedelta(weeks=index), 7) for index in range(7)]
        self.assertEqual(len(set(slots)), 7)
        self.assertEqual(slots[0], weekly_index(start + timedelta(weeks=7), 7))
        self.assertEqual(slots[0], weekly_index(start + timedelta(days=6), 7))
        with self.assertRaises(ValueError):
            weekly_index(start, 0)

    def test_monthly_historical_runtime_replaces_not_duplicates_current_selection(self):
        for day, runtime in ((date(2026, 11, 2), "live-runtime"), (date(2026, 11, 9), "runtime-current")):
            suites = scheduled_suites(day)
            self.assertEqual(suites[-1], runtime)
            self.assertEqual(len(set(suites)), len(suites))
            self.assertTrue(set(suites) <= SUITES.keys())
            self.assertIn("import-corpus", suites)
            self.assertIn("pinned-import", suites)
            self.assertIn("live-curseforge", suites)
            self.assertNotIn("import-survey", suites)

    def test_plan_only_subprocess_has_no_execution_claim(self):
        with tempfile.TemporaryDirectory() as temp:
            result = subprocess.run([sys.executable, str(SCRIPTS / "check-live-compatibility.py"),
                "--plan-only", "--output", temp], capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            plan = json.loads((Path(temp) / "selection.json").read_text())
            self.assertEqual(plan["execution"], "NOT_RUN")
            self.assertFalse(plan["qualification"])
            self.assertEqual(plan["product_profile"], "release")
            self.assertEqual(plan["suites"], scheduled_suites(date.fromisoformat(plan["day_utc"])))
            self.assertFalse((Path(temp) / "runs").exists())

    def test_shared_runner_failure_is_returned_once_without_retry_or_pass_record(self):
        with tempfile.TemporaryDirectory() as temp:
            with patch.object(sys, "argv", ["check-live-compatibility.py", "--output", temp]), patch.object(runner.subprocess, "call", return_value=2) as execute:
                self.assertEqual(runner.main(), 2)
                execute.assert_called_once()
            command = execute.call_args.args[0]
            self.assertEqual(command[:2], [sys.executable, str(SCRIPTS / "verify.py")])
            plan = json.loads((Path(temp) / "selection.json").read_text())
            self.assertEqual(command[2:2 + len(plan["suites"])], plan["suites"])
            self.assertEqual(command[-8:-2], ["--product-features", "telemetry", "--product-profile", "release", "--test-features", "all"])
            self.assertFalse(plan["qualification"])
            self.assertEqual(plan["execution"], "NOT_RUN")


if __name__ == "__main__":
    unittest.main()
