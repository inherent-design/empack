"""Offline contracts for the native CLI smoke driver; live fixtures remain separate."""
import importlib.util
import hashlib
import json
import subprocess
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("empack_import_smoke", Path(__file__).parents[1] / "survey-imports.py")
smoke = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = smoke
SPEC.loader.exec_module(smoke)


class NativeSmokeContracts(unittest.TestCase):
    def test_legacy_survey_entrypoint_preserves_help_without_running_live_work(self):
        for name in ("survey-imports.py", "import-smoke-test.py"):
            with self.subTest(name=name):
                result = subprocess.run([sys.executable, str(Path(__file__).parents[1] / name), "--help"],
                                        capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("not pinned regression qualification", result.stdout)
                self.assertIn("--profile", result.stdout)

    def test_survey_report_retains_observations_without_becoming_qualification(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "empack"
            binary.write_bytes(b"fixture executable")
            layout = smoke.RuntimeLayout(root, root / "packs", root / "projects", root / "cache", root / "report.json")
            observations = [{"import_result": {"success": False, "exit_code": 1}, "failure_reason": "upstream drift"}]
            smoke.save_survey_report(observations, layout, "curated-latest", binary)
            report = json.loads(layout.report_path.read_text())
            self.assertEqual(report["kind"], "rolling-import-survey")
            self.assertFalse(report["qualification"])
            self.assertEqual(report["selection"], "curated-latest")
            self.assertEqual(report["entries"], observations)
            self.assertGreater(report["observed_at_unix"], 0)
            self.assertEqual(report["executable"]["sha256"], hashlib.sha256(binary.read_bytes()).hexdigest())

    def test_native_errors_remain_visible_after_cursor_restoration(self):
        result = smoke.parse_import_output("\x1b[?25hError: source changed\r\n", "")
        self.assertEqual(result.warnings, ["Error: source changed"])
        self.assertFalse(result.success)

    def test_only_exact_saved_restricted_obligations_allow_manual_downloads(self):
        diagnostic = ('Import input "provider:curseforge:42:456:mod.jar": RestrictedDownload\n'
                      'Import continuation was saved\n1 content obligations need explicit input')
        self.assertEqual(smoke.pending_import_associations(diagnostic), [("provider:curseforge:42:456:mod.jar", 42, 456)])
        for bad in (diagnostic.replace("was saved", "failed"), diagnostic.replace("1 content", "2 content"),
                    diagnostic.replace("RestrictedDownload", "UnsupportedTransport"), "Download deadline elapsed"):
            with self.subTest(bad=bad), self.assertRaises(RuntimeError):
                smoke.pending_import_associations(bad)

    def test_relative_executable_is_bound_before_child_workdir_changes(self):
        with tempfile.TemporaryDirectory() as root:
            executable = Path(root) / "empack"
            executable.touch()
            previous = Path.cwd()
            try:
                os.chdir(root)
                with patch.object(sys, "argv", ["survey-imports.py", "--profile", "curated", "--empack-bin", "empack"]), patch.object(smoke, "run_curated_mode", return_value=0) as run:
                    with self.assertRaises(SystemExit) as result:
                        smoke.main()
                    self.assertEqual(result.exception.code, 0)
                    self.assertEqual(run.call_args.args[0], executable.resolve())
            finally:
                os.chdir(previous)

    def test_generic_import_failure_does_not_become_a_continuation(self):
        with tempfile.TemporaryDirectory() as root:
            base = Path(root)
            layout = smoke.RuntimeLayout(base, base / "packs", base / "projects", base / "cache", base / "report.json")
            failed = smoke.CommandResult(stderr="Download deadline elapsed", exit_code=1)
            with patch.object(smoke, "run_empack_command", return_value=failed) as command, patch.object(smoke, "urlopen") as download:
                result, continued = smoke.run_curated_import(base / "empack", base / "source.zip", base / "project", layout, "fixture", False)
                self.assertFalse(result["success"])
                self.assertFalse(continued)
                self.assertIn("Download deadline elapsed", result["output_tail"])
                command.assert_called_once()
                download.assert_not_called()

    def test_fixture_world_choice_is_passed_explicitly_to_import(self):
        with tempfile.TemporaryDirectory() as root:
            base = Path(root)
            layout = smoke.RuntimeLayout(base, base / "packs", base / "projects", base / "cache", base / "report.json")
            with patch.object(smoke, "run_empack_command", return_value=smoke.CommandResult(success=True)) as command:
                result, continued = smoke.run_curated_import(base / "empack", base / "source.zip", base / "project", layout, "worlds", False, "saves")
                self.assertTrue(result["success"])
                self.assertFalse(continued)
                args = command.call_args.args[1]
                self.assertEqual(args[args.index("--world-folder") + 1], "saves")

    def test_native_build_failure_is_reported_without_retry_or_legacy_fallback(self):
        with tempfile.TemporaryDirectory() as root:
            base = Path(root)
            for name in ("empack.yml", "empack.lock"):
                (base / name).write_bytes(b"original")
            layout = smoke.RuntimeLayout(base, base / "packs", base, base / "cache", base / "report.json")
            responses = [smoke.CommandResult(success=True), smoke.CommandResult(success=True), smoke.CommandResult(stderr="not verified", exit_code=1)]
            with patch.object(smoke, "run_empack_command", side_effect=responses) as command:
                result = smoke.run_curated_build(smoke.CURATED_PACKS[0], base, base / "empack", layout, 10, False)
                self.assertFalse(result.success)
                self.assertEqual(command.call_count, 3)
                self.assertIn("not verified", result.output_tail)
                self.assertEqual(command.call_args.args[1], ["--yes", "build", "--ad-hoc", "prism", "--delivery", "bundled", "--updates", "snapshot", "--format", "zip"])

    def test_sync_document_drift_fails_before_build(self):
        for document in ("empack.yml", "empack.bindings.yml", "empack.lock"):
            with self.subTest(document=document), tempfile.TemporaryDirectory() as root:
                base = Path(root)
                for name in ("empack.yml", "empack.bindings.yml", "empack.lock"):
                    (base / name).write_bytes(b"original")
                layout = smoke.RuntimeLayout(base, base / "packs", base, base / "cache", base / "report.json")
                def changed(*args, **kwargs):
                    (base / document).write_bytes(b"changed")
                    return smoke.CommandResult(success=True)
                with patch.object(smoke, "run_empack_command", side_effect=changed) as command, self.assertRaisesRegex(RuntimeError, "sync changed"):
                    smoke.run_curated_build(smoke.CURATED_PACKS[0], base, base / "empack", layout, 10, False)
                command.assert_called_once()

    def test_sync_cannot_create_or_remove_bindings_unnoticed(self):
        for existed in (False, True):
            with self.subTest(existed=existed), tempfile.TemporaryDirectory() as root:
                base = Path(root)
                for name in ("empack.yml", "empack.lock"):
                    (base / name).write_bytes(b"original")
                bindings = base / "empack.bindings.yml"
                if existed:
                    bindings.write_bytes(b"original")
                layout = smoke.RuntimeLayout(base, base / "packs", base, base / "cache", base / "report.json")
                def changed(*args, **kwargs):
                    if existed:
                        bindings.unlink()
                    else:
                        bindings.write_bytes(b"new")
                    return smoke.CommandResult(success=True)
                with patch.object(smoke, "run_empack_command", side_effect=changed) as command, self.assertRaisesRegex(RuntimeError, "sync changed"):
                    smoke.run_curated_build(smoke.CURATED_PACKS[0], base, base / "empack", layout, 10, False)
                command.assert_called_once()


if __name__ == "__main__":
    unittest.main()
