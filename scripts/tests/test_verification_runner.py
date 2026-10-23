"""Exercise discovery/result joins and command failures, not fabricated passing runs."""
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from verification.evidence import read_results, selected_tests
from verification.process import redact, run


class SuiteEvidence(unittest.TestCase):
    def inventory(self, ignored=False):
        return {"test-count": 1, "rust-suites": {"pkg::cli": {
            "status": "listed", "binary-name": "cli", "testcases": {
                "checks_output": {"ignored": ignored, "filter-match": {"status": "matches"}}}}}}

    def test_required_binary_and_case_are_discovered(self):
        selected, all_ids = selected_tests(self.inventory(), False, ("cli",))
        self.assertEqual(selected, ["pkg::cli::checks_output"])
        self.assertEqual(all_ids, set(selected))

    def test_required_case_retains_module_path(self):
        inventory = self.inventory()
        cases = inventory["rust-suites"]["pkg::cli"]["testcases"]
        cases["module::checks_output"] = cases.pop("checks_output")
        selected, _ = selected_tests(inventory, False, ("cli",), ("module::checks_output",))
        self.assertEqual(selected, ["pkg::cli::module::checks_output"])

    def test_empty_or_ignored_selection_is_not_success(self):
        for inventory, ignored, binaries in [
            (self.inventory(), False, ("misspelled",)),
            (self.inventory(True), False, ("cli",)),
        ]:
            with self.subTest(binaries=binaries, ignored=ignored), self.assertRaisesRegex(ValueError, "Empty required"):
                selected_tests(inventory, ignored, binaries)

    def test_selected_ignored_case_requires_explicit_selection(self):
        self.assertEqual(len(selected_tests(self.inventory(True), True, ("cli",))[0]), 1)

    def test_unlisted_binary_fails_discovery(self):
        inventory = self.inventory()
        inventory["rust-suites"]["pkg::cli"]["status"] = "skipped"
        with self.assertRaisesRegex(ValueError, "Unlisted"):
            selected_tests(inventory, False)

    def test_exact_suite_filter_can_exclude_unrelated_binaries(self):
        inventory = self.inventory()
        inventory["rust-suites"]["other"] = {
            "status": "skipped", "binary-name": "other", "testcases": {}}
        self.assertEqual(len(selected_tests(inventory, False, ("cli",))[0]), 1)
        with self.assertRaisesRegex(ValueError, "Unlisted"):
            selected_tests(inventory, False, ("cli", "other"))

    def results(self, cases, count=1):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "junit.xml"
            path.write_text(f'<testsuites tests="{count}"><testsuite name="pkg::cli">{cases}</testsuite></testsuites>')
            return read_results(path, ["pkg::cli::checks_output"], {"pkg::cli::checks_output"})

    def case(self, child=""):
        return f'<testcase classname="pkg::cli" name="checks_output">{child}</testcase>'

    def test_actual_success_is_pass(self):
        self.assertEqual(self.results(self.case())[0]["status"], "PASS")

    def test_skip_and_absence_are_not_pass(self):
        self.assertEqual(self.results(self.case('<skipped/>'))[0]["status"], "NOT_RUN")
        self.assertEqual(self.results("", count=0)[0]["status"], "NOT_RUN")

    def test_failure_error_and_flake_remain_failures(self):
        for child in ("failure", "error", "flakyFailure", "rerunError"):
            with self.subTest(child=child):
                self.assertEqual(self.results(self.case(f'<{child}/>'))[0]["status"], "FAIL")

    def test_duplicate_and_unknown_results_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            self.results(self.case() * 2, count=2)
        with self.assertRaisesRegex(ValueError, "undiscovered"):
            self.results(self.case().replace('name="checks_output"', 'name="wrong_test"'))

    def test_inconsistent_count_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "count mismatch"):
            self.results(self.case(), count=2)

    def test_unrecognized_result_cannot_be_a_pass(self):
        with self.assertRaisesRegex(ValueError, "Unknown test result"):
            self.results(self.case('<unknown-outcome/>'))


class CommandEvidence(unittest.TestCase):
    def test_unicode_command_logs_ignore_the_host_file_encoding(self):
        from unittest.mock import patch
        original = Path.write_text
        def windows_encoding(path, data, encoding=None, errors=None, newline=None):
            return original(path, data, encoding=encoding or "cp1252", errors=errors, newline=newline)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            text = "Nextest ──── 日誌\n"
            with patch.object(Path, "write_text", windows_encoding):
                status, output, expired = run(
                    [sys.executable, "-c", "import os; os.write(1, " + repr(text.encode("utf-8")) + ")"],
                    cwd=root, env=dict(os.environ), log=root / "log", timeout=10)
            self.assertEqual(status, 0)
            self.assertFalse(expired)
            self.assertEqual(output, text)
            self.assertEqual((root / "log").read_bytes(), text.encode("utf-8"))

    def test_credentials_and_url_queries_are_redacted(self):
        text = 'token=secret-value https://user:pass@example.org/download?signature=private#anchor'
        clean = redact(text, {"PROVIDER_TOKEN": "secret-value"})
        for private in ('secret-value', 'user:pass', 'signature=private', 'anchor'):
            self.assertNotIn(private, clean)
        self.assertIn('https://example.org/download', clean)

    def test_nonzero_status_and_separate_stdout_are_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            status, output, expired = run(
                [sys.executable, '-c', 'import sys; print("data"); print("diagnostic", file=sys.stderr); sys.exit(7)'],
                cwd=root, env=dict(os.environ), log=root / 'log', timeout=10, separate_stdout=True)
            self.assertEqual(status, 7)
            self.assertEqual(output.strip(), 'data')
            self.assertFalse(expired)
            self.assertIn('diagnostic', (root / 'log').read_text())

    def test_timeout_does_not_retry_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            code = 'from pathlib import Path; import time; p=Path("attempts"); p.write_text(p.read_text()+"x" if p.exists() else "x"); time.sleep(30)'
            status, _, expired = run([sys.executable, '-c', code], cwd=root,
                env=dict(os.environ), log=root / 'log', timeout=1)
            self.assertTrue(expired)
            self.assertNotEqual(status, 0)
            self.assertEqual((root / 'attempts').read_text(), 'x')

    def test_large_output_is_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            status, output, expired = run(
                [sys.executable, '-c', 'print("x" * (5 * 1024 * 1024))'],
                cwd=root, env=dict(os.environ), log=root / 'log', timeout=10)
            self.assertEqual(status, 0)
            self.assertFalse(expired)
            self.assertLess(len(output), 4 * 1024 * 1024)
            self.assertIn('output truncated', output)


class AttemptPolicy(unittest.TestCase):
    def test_java_home_selects_the_probed_executable_without_path_fallback(self):
        from unittest.mock import patch
        from verify import Blocked, java_probe
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            java = root / "jdk" / "bin" / ("java.exe" if os.name == "nt" else "java")
            java.parent.mkdir(parents=True)
            java.write_bytes(b"fixture executable identity")
            environment = {"JAVA_HOME": str(java.parent.parent), "PATH": "wrong-java"}
            def checked(command, attempt, name, env, timeout):
                self.assertEqual(command, [str(java.resolve()), "-version"])
                self.assertEqual(env, environment)
                (attempt / (name + ".log")).write_text('openjdk version "21.0.13"\n')
            with patch("verify.checked", side_effect=checked):
                result = java_probe(root, environment)
            self.assertEqual(result["path"], str(java.resolve()))
            self.assertIn("21.0.13", result["version"])
            java.unlink()
            with self.assertRaisesRegex(Blocked, "Java prerequisite unavailable"):
                java_probe(root, environment)

    def test_empty_and_inapplicable_runs_are_not_passes(self):
        from verify import aggregate_status
        self.assertEqual(aggregate_status([]), 'NOT_RUN')
        self.assertEqual(aggregate_status([{'status': 'NOT_APPLICABLE'}]), 'NOT_APPLICABLE')
        self.assertEqual(aggregate_status([{'status': 'PASS'}, {'status': 'BLOCKED'}]), 'BLOCKED')
        self.assertEqual(aggregate_status([{'status': 'FAIL'}, {'status': 'PASS'}]), 'FAIL')

    def test_external_artifact_requires_complete_identity(self):
        from types import SimpleNamespace
        from verify import Blocked, product
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(binary='relative', sha256=None, version=None,
                                   target=None, product_features=None, product_profile=None)
            with self.assertRaisesRegex(Blocked, 'requires --sha256'):
                product(args, Path(directory), {}, 'host')

    def test_wrong_digest_and_version_cannot_qualify(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        from verification.evidence import sha256
        from verify import Blocked, product
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'empack'
            binary.write_bytes(b'fixture binary; never executed')
            args = SimpleNamespace(binary=str(binary), sha256='0' * 64, version='1',
                                   target='fixture-target', product_features='default', product_profile='dev')
            with self.assertRaisesRegex(Blocked, 'SHA-256'):
                product(args, root, {}, 'host')
            args.sha256 = sha256(binary)
            with patch('verify.checked', return_value='empack 2\n'):
                with self.assertRaisesRegex(Blocked, 'expected version'):
                    product(args, root, {}, 'host')

    def test_release_profile_builds_and_executes_the_separate_product(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        from verify import product
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "empack/src").mkdir(parents=True)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["empack"]\nresolver = "2"\n'
                '[workspace.package]\nversion = "1.2.3"\n')
            (root / "Cargo.lock").write_text(
                'version = 4\n[[package]]\nname = "empack"\nversion = "1.2.3"\n')
            (root / "empack/Cargo.toml").write_text(
                '[package]\nname = "empack"\nversion.workspace = true\nedition = "2024"\n')
            (root / "empack/src/main.rs").write_text(
                'fn main() { println!("empack {}", env!("CARGO_PKG_VERSION")); }\n')
            attempt = root / "attempt"
            attempt.mkdir()
            args = SimpleNamespace(binary=None, sha256=None, version=None, target=None,
                                   product_features="default", product_profile="release", suites=["cli"])
            with patch("verify.ROOT", root):
                artifact = product(args, attempt, dict(os.environ), "fixture-host")
            self.assertEqual(artifact["profile"], "release")
            self.assertEqual(Path(artifact["path"]).parent.name, "release")
            self.assertEqual(artifact["version"], "1.2.3")
            self.assertIn("release", (attempt / "product-build.log").read_text())
            self.assertEqual((attempt / "product-version.log").read_text().strip(), "empack 1.2.3")

    def test_malformed_xml_is_not_a_command_success(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'junit.xml'
            path.write_text('<testsuites')
            with self.assertRaisesRegex(ValueError, 'Malformed JUnit'):
                read_results(path, ['test'], {'test'})


class SourceIdentity(unittest.TestCase):
    def test_staging_deletion_is_stable_but_new_source_changes_identity(self):
        import subprocess
        from unittest.mock import patch
        from verify import source_snapshot
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(['git', 'init', '--quiet', str(root)], check=True)
            source = root / 'source.rs'
            source.write_text('original')
            subprocess.run(['git', 'add', 'source.rs'], cwd=root, check=True)
            attempt = root / 'attempt'
            attempt.mkdir()
            with patch('verify.ROOT', root):
                original = source_snapshot(attempt, dict(os.environ))
                source.unlink()
                deleted = source_snapshot(attempt, dict(os.environ))
                self.assertNotEqual(original, deleted)
                subprocess.run(['git', 'add', '-u'], cwd=root, check=True)
                self.assertEqual(deleted, source_snapshot(attempt, dict(os.environ)))
                source.write_text('different')
                changed = source_snapshot(attempt, dict(os.environ))
                self.assertNotEqual(original, changed)
                self.assertNotEqual(deleted, changed)
