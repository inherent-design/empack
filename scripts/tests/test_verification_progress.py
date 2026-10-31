"""Observe fixed phase markers without launching a product or compiler."""
from contextlib import redirect_stdout
from datetime import datetime
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import verify
from verification import progress
from verification.receipts import inspect_suite_receipt


class VerificationProgress(unittest.TestCase):
    def test_entry_precedes_work_and_return_is_not_acceptance(self):
        with tempfile.TemporaryDirectory() as directory, redirect_stdout(io.StringIO()) as console:
            root = Path(directory)
            path = root / "discovery.progress.json"
            with progress.phase(root, "discovery"):
                entered = json.loads(path.read_text())
                self.assertEqual(entered["state"], "entered")
                self.assertNotIn("finished_at", entered)
            returned = json.loads(path.read_text())
            self.assertEqual(returned["state"], "returned")
            self.assertEqual(returned["started_at"], entered["started_at"])
            self.assertIsNotNone(datetime.fromisoformat(returned["finished_at"]).tzinfo)
            self.assertGreaterEqual(returned["elapsed_seconds"], 0)
            self.assertFalse(returned["qualification"])
            self.assertNotIn("status", returned)
            self.assertEqual(returned["kind"], "verification-progress")
            self.assertIn("discovery: entered", console.getvalue())
            with self.assertRaisesRegex(ValueError, "unrelated suite attempt"):
                inspect_suite_receipt(path, ["cli"], "x86_64-unknown-linux-gnu", "a" * 40)

    def test_errors_and_interrupts_preserve_the_original_cause_without_details(self):
        secret = "secret fixture credential and command arguments"
        for error, state in [(RuntimeError(secret), "raised"), (KeyboardInterrupt(secret), "interrupted")]:
            with self.subTest(state=state), tempfile.TemporaryDirectory() as directory, redirect_stdout(io.StringIO()) as console:
                root = Path(directory)
                with self.assertRaises(type(error)) as raised:
                    with progress.phase(root, "execution"):
                        raise error
                self.assertIs(raised.exception, error)
                contents = (root / "execution.progress.json").read_text()
                self.assertEqual(json.loads(contents)["state"], state)
                self.assertNotIn(secret, contents)
                self.assertNotIn(secret, console.getvalue())

    def test_diagnostic_storage_and_console_failures_do_not_override_work(self):
        with tempfile.TemporaryDirectory() as directory:
            error = RuntimeError("preserve actual outcome")
            with patch.object(progress, "write_json", side_effect=OSError("unavailable")), \
                    patch("builtins.print", side_effect=BrokenPipeError("closed console")):
                with progress.phase(Path(directory), "execution"):
                    pass
                with self.assertRaises(RuntimeError) as raised:
                    with progress.phase(Path(directory), "execution"):
                        raise error
                self.assertIs(raised.exception, error)
            closed = io.StringIO()
            closed.close()
            with redirect_stdout(closed):
                with progress.phase(Path(directory), "execution"):
                    pass
            with redirect_stdout(closed), self.assertRaises(RuntimeError) as raised:
                with progress.phase(Path(directory), "execution"):
                    raise error
            self.assertIs(raised.exception, error)
            self.assertEqual(json.loads((Path(directory) / "execution.progress.json").read_text())["state"], "raised")

    def test_checked_command_keeps_output_deadline_and_nonzero_classification(self):
        with tempfile.TemporaryDirectory() as directory, redirect_stdout(io.StringIO()) as console:
            root = Path(directory)
            for code, timed_out in [(7, False), (0, True)]:
                def observed_run(command, **options):
                    self.assertEqual(command, ["inert-command", "private argument"])
                    self.assertEqual(options["timeout"], 1800)
                    self.assertTrue(options["separate_stdout"])
                    self.assertEqual(json.loads((root / "discovery.progress.json").read_text())["state"], "entered")
                    return code, "private child output", timed_out
                with patch.object(verify, "run", side_effect=observed_run), self.assertRaisesRegex(RuntimeError, f"exit {code}, timeout={timed_out}"):
                    verify.checked(["inert-command", "private argument"], root, "discovery", {}, timeout=1800)
            record = json.loads((root / "discovery.progress.json").read_text())
            self.assertEqual(record["state"], "returned")
            self.assertNotIn("status", record)
            self.assertNotIn("private", console.getvalue())

    def test_unavailable_tool_retains_blocked_classification(self):
        with tempfile.TemporaryDirectory() as directory, redirect_stdout(io.StringIO()):
            root = Path(directory)
            with patch.object(verify, "run", side_effect=FileNotFoundError("missing")), \
                    self.assertRaisesRegex(verify.Blocked, "Required command unavailable"):
                verify.checked(["inert-command"], root, "compiler", {})
            self.assertEqual(json.loads((root / "compiler.progress.json").read_text())["state"], "raised")
