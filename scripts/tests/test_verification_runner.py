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
            return original(path, data, encoding=encoding or "cp1252", errors=errors,
                            newline="\r\n" if newline is None else newline)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for ending in ("\n", "\r\n"):
                text = "Nextest ──── 日誌" + ending
                with self.subTest(ending=ending), patch.object(Path, "write_text", windows_encoding):
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

    def test_registry_package_identity_survives_without_preserving_url_credentials(self):
        identity = 'registry+https://github.com/rust-lang/crates.io-index#semver@1.0.28'
        self.assertEqual(redact(identity, {}), identity)
        text = 'registry+https://user:password@example.org/index?token=private#semver@1.0.28'
        clean = redact(text, {"ACCESS_TOKEN": "private"})
        self.assertIn('#semver@1.0.28', clean)
        for secret in ('user:password', 'token=private'):
            self.assertNotIn(secret, clean)
        ordinary = redact('https://example.org/index#semver@1.0.28', {})
        self.assertNotIn('#', ordinary)
        compound = redact('https://user:pass@example.org/?token=known-secret&signature=other-secret#private-fragment',
                          {"TOKEN": "known-secret"})
        for secret in ('known-secret', 'other-secret', 'private-fragment', 'user:pass'):
            self.assertNotIn(secret, compound)
        overlapping = redact('known-secret EMPACK __EMPACK_LOG_REDACTED__',
                             {"TOKEN": "known-secret", "KEY": "EMPACK"})
        self.assertEqual(overlapping, '<redacted> <redacted> __<redacted>_LOG_REDACTED__')

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
    def test_windows_compiler_discovery_survives_without_host_cargo_or_application_state(self):
        from unittest.mock import patch
        from verify import isolated_environment
        home = Path("isolated-home")
        toolset = {"PROGRAMFILES(X86)": "installed-vswhere-root", "ProgramFiles": "installed-programs",
                   "VSINSTALLDIR": "installed-vs", "VCINSTALLDIR": "installed-vc",
                   "VCToolsVersion": "14.44", "VSCMD_ARG_TGT_ARCH": "x64",
                   "VSCMD_ARG_VCVARS_SPECTRE": "spectre", "WindowsSdkDir": "installed-sdk",
                   "WindowsSDKVersion": "10.0", "LIB": "installed-libs", "LIBPATH": "installed-libpath",
                   "INCLUDE": "installed-includes"}
        hostile = {"HOME": "host-home", "CARGO_HOME": "host-cargo", "RUSTC_WRAPPER": "host-wrapper",
                   "CARGO_REGISTRIES_CRATES_IO_TOKEN": "host-token", "RUSTFLAGS": "host-flags",
                   "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER": "host-linker",
                   "EMPACK_STATE_DIR": "host-state", "UNRELATED": "host-value"}
        with patch.dict(os.environ, toolset | hostile, clear=True), patch("verify.os.name", "nt"):
            worker = isolated_environment(home, compiler=True)
            runtime = isolated_environment(home)
        worker_tools = {key.upper(): value for key, value in worker.items()}
        runtime_tools = {key.upper(): value for key, value in runtime.items()}
        for key, value in toolset.items():
            self.assertEqual(worker_tools[key.upper()], value)
            self.assertNotIn(key.upper(), runtime_tools)
        self.assertEqual(worker["HOME"], str(home))
        self.assertEqual(worker["EMPACK_STATE_DIR"], str(home))
        for key in hostile.keys() - {"HOME", "EMPACK_STATE_DIR"}:
            self.assertNotIn(key, worker)
        with tempfile.TemporaryDirectory() as temp:
            installation = Path(temp)
            native = installation / "bin/Hostx64/x64"
            native.mkdir(parents=True)
            for tool in ("cl.exe", "link.exe"):
                (native / tool).write_bytes(b"inert installed-tool fixture")
            environment = toolset | hostile | {"VCToolsInstallDir": str(installation), "PATH": "git-tools"}
            with patch.dict(os.environ, environment, clear=True), patch("verify.os.name", "nt"):
                worker = isolated_environment(home, compiler=True)
                runtime = isolated_environment(home)
                self.assertEqual(worker["PATH"], str(native) + os.pathsep + "git-tools")
                self.assertEqual(runtime["PATH"], "git-tools")
                (native / "link.exe").unlink()
                with self.assertRaisesRegex(RuntimeError, "MSVC compiler tools are missing"):
                    isolated_environment(home, compiler=True)

    def test_standard_suite_supplies_bound_inventory_to_test_process(self):
        import hashlib
        import json
        from unittest.mock import patch
        from verify import run_suite
        from verification.suites import Suite
        inventory = SuiteEvidence().inventory()
        artifact = dict(path="/ordinary/product", sha256="0" * 64,
                        version="0.0.0-dev", features="default", target="aarch64-apple-darwin")
        with tempfile.TemporaryDirectory() as directory:
            record = {}
            with patch("verify.checked", return_value=json.dumps(inventory)), \
                    patch("verify.compare_rust_inventory"), \
                    patch("verify.run", return_value=(1, "retained failure", False)) as worker:
                run_suite("deterministic", Suite(), Path(directory), {}, artifact, record, "minimal")
            environment = worker.call_args.kwargs["env"]
            selected = Path(environment["EMPACK_TEST_INVENTORY"])
            self.assertTrue(selected.is_absolute())
            self.assertEqual(json.loads(selected.read_text()), inventory)
            self.assertEqual(environment["EMPACK_TEST_INVENTORY_SHA256"],
                             hashlib.sha256(selected.read_bytes()).hexdigest())
            self.assertEqual(environment["EMPACK_E2E_BIN"], artifact["path"])
            self.assertEqual(record["status"], "FAIL", "fixture metadata cannot turn failed execution into a pass")

    def test_python_worker_builds_offline_with_installed_tools_and_isolated_state(self):
        import subprocess
        from unittest.mock import patch
        from verify import python_environment
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "isolated home"
            home.mkdir()
            attempt = root / "attempt"
            attempt.mkdir()
            with patch.dict(os.environ, {"CARGO_HOME": str(root / "host-cargo"),
                                        "CARGO_REGISTRIES_CRATES_IO_TOKEN": "must-not-reach-worker",
                                        "RUSTC_WRAPPER": "untrusted-wrapper",
                                        "EMPACK_STATE_DIR": str(root / "host-state")}):
                worker, tools = python_environment(home, attempt, dict(os.environ))
            self.assertEqual(worker["HOME"], str(home))
            self.assertEqual(worker["EMPACK_STATE_DIR"], str(home))
            self.assertEqual(worker["CARGO_HOME"], str(home / ".cargo"))
            self.assertEqual(worker["RUSTUP_AUTO_INSTALL"], "0")
            self.assertNotIn("CARGO_REGISTRIES_CRATES_IO_TOKEN", worker)
            self.assertNotIn("RUSTC_WRAPPER", worker)
            self.assertFalse(tools["automatic_installation"])
            (root / "src").mkdir()
            (root / "Cargo.toml").write_text(
                '[package]\nname = "isolated-probe"\nversion = "0.0.0"\nedition = "2024"\n[workspace]\n')
            (root / "Cargo.lock").write_text(
                'version = 4\n[[package]]\nname = "isolated-probe"\nversion = "0.0.0"\n')
            (root / "src/main.rs").write_text('fn main() { println!("isolated compiler works"); }\n')
            result = subprocess.run(["cargo", "run", "--locked", "--offline", "--quiet"],
                                    cwd=root, env=worker, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "isolated compiler works")
            self.assertFalse((home / ".rustup").exists())
            self.assertFalse((root / "host-cargo").exists())
            self.assertFalse((root / "host-state").exists())

    def test_python_worker_missing_toolchain_is_blocked_without_installing(self):
        from unittest.mock import patch
        from verify import Blocked, python_environment
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "home"
            home.mkdir()
            (root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "99.99.99"\n')
            with patch("verify.ROOT", root), self.assertRaisesRegex(Blocked, "automatic installation is disabled"):
                python_environment(home, root, dict(os.environ))
            diagnostic = (root / "python-rustc.log").read_text()
            self.assertIn("not installed", diagnostic)
            self.assertNotIn("downloading", diagnostic)
            self.assertFalse((home / ".rustup").exists())

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
        from verify import run_suite
        from verification.suites import SUITES
        suite = SUITES["native-remount"]
        self.assertTrue(suite.ignored)
        from verification.suites import nextest_command
        command = nextest_command("run", suite, Path("fixture.toml"), "minimal")
        self.assertEqual(command[-2:], ["--test-threads", "1"], "native fixtures share device allocation")
        self.assertIsNone(suite.inapplicable_reason("aarch64-apple-darwin"))
        for target in ("x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"):
            record = {}
            run_suite("native-remount", suite, Path("unused"), {}, {"target": target}, record, "minimal")
            self.assertEqual(record["status"], "NOT_APPLICABLE")
            self.assertNotIn("tests", record)
            self.assertEqual(aggregate_status([record]), "NOT_APPLICABLE")

    def test_failure_and_blocked_prerequisites_remain_visible_with_unstarted_suites(self):
        from verify import aggregate_status
        self.assertEqual(aggregate_status([{'status': 'FAIL'}, {'status': 'NOT_RUN'}]), 'FAIL')
        self.assertEqual(aggregate_status([{'status': 'BLOCKED'}, {'status': 'NOT_RUN'}]), 'BLOCKED')
        with self.assertRaises(ValueError):
            aggregate_status([{'status': 'FAIL'}, {'status': 'unknown'}])

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
                '[workspace]\nmembers = ["empack", "empack-core", "empack-lib"]\nresolver = "2"\n'
                '[workspace.package]\nversion = "1.2.3"\n')
            (root / "empack/Cargo.toml").write_text(
                '[package]\nname = "empack"\nversion.workspace = true\nedition = "2024"\n'
                '[dependencies]\nempack-lib = { path = "../empack-lib" }\n')
            for name, dependency in (("empack-core", ""), ("empack-lib", '[dependencies]\nempack-core = {path = "../empack-core"}\n')):
                (root / name / "src").mkdir(parents=True)
                (root / name / "src/lib.rs").write_text("")
                (root / name / "Cargo.toml").write_text(
                    f'[package]\nname = "{name}"\nversion.workspace = true\nedition = "2024"\n' + dependency)
            (root / "empack/src/main.rs").write_text(
                'fn main() { println!("empack {}", env!("CARGO_PKG_VERSION")); }\n')
            import subprocess
            lock = subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, capture_output=True, text=True)
            self.assertEqual(lock.returncode, 0, lock.stderr)
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
