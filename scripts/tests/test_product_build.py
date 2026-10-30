"""Actual tiny Cargo builds and malformed evidence; not empack package acceptance."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from verification.evidence import sha256
from verification.product_build import inspect_build, inspect_retained_build
from verify import product


def fixture(root, leak=""):
    (root / "Cargo.toml").write_text(
        '[workspace]\nmembers = ["empack", "empack-core", "empack-lib", "empack-tests", "bridge"]\n'
        'resolver = "2"\n[workspace.package]\nversion = "1.2.3"\n')
    manifests = {
        "empack": '[features]\ntelemetry = ["empack-lib/telemetry"]\n'
                  '[dependencies]\nempack-lib = {path = "../empack-lib"}\nbridge = {path = "../bridge"}\n',
        "empack-core": "",
        "empack-lib": '[features]\ndefault = []\ntest-utils = []\ntelemetry = []\n'
                      '[dependencies]\nempack-core = {path = "../empack-core"}\n',
        "empack-tests": "",
        "bridge": ('[dependencies]\nempack-lib = {path = "../empack-lib", features = ["test-utils"]}\n'
                   if leak == "feature" else '[dependencies]\nempack-tests = {path = "../empack-tests"}\n'
                   if leak == "package" else ""),
    }
    for name, manifest in manifests.items():
        (root / name / "src").mkdir(parents=True)
        (root / name / "Cargo.toml").write_text(
            f'[package]\nname = "{name}"\nversion.workspace = true\nedition = "2024"\n' + manifest)
        (root / name / "src" / ("main.rs" if name == "empack" else "lib.rs")).write_text(
            'fn main() { println!("empack {}", env!("CARGO_PKG_VERSION")); }' if name == "empack" else "")
    subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, check=True, capture_output=True)


def build(root, name="attempt", features="default"):
    attempt = root / name
    attempt.mkdir()
    args = SimpleNamespace(binary=None, sha256=None, version=None, target=None,
                           product_features=features, product_profile="dev", suites=["cli"])
    with patch("verify.ROOT", root):
        return product(args, attempt, dict(os.environ), "fixture-host")


def commit_source(root):
    (root / "rust-toolchain.toml").write_bytes((Path(__file__).parents[2] / "rust-toolchain.toml").read_bytes())
    (root / ".gitignore").write_text("/target/\n")
    for arguments in (("init", "-q"), ("config", "user.name", "Compiler fixture"),
                      ("config", "user.email", "fixture@example.invalid"), ("add", "."),
                      ("-c", "commit.gpgSign=false", "-c", "core.hooksPath=" + os.devnull, "commit", "-qm", "fixture")):
        subprocess.run(["git", *arguments], cwd=root, check=True, capture_output=True)


def native_compiler():
    lines = subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
    return (next(line.removeprefix("host: ") for line in lines if line.startswith("host: ")),
            next(line.removeprefix("release: ") for line in lines if line.startswith("release: ")))


class CompilerOutputEvidence(unittest.TestCase):
    def test_observed_cross_prelude_is_replayed_from_retained_stdout(self):
        from compiled_product_fixture import compilation
        from verification.product_build import inspect_compilation
        target = "aarch64-unknown-linux-gnu"
        banner = "\n  1.99.1-x86_64-unknown-linux-gnu unchanged - rustc 1.99.1 (58cf8f9c3 2026-10-13)\n\n"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            compilation(root, target, "c" * 64, "a" * 40, {}, builder="cross")
            for name in ("build.stdout", "metadata.stdout"):
                path = root / name
                path.write_text(banner + path.read_text(), encoding="utf-8")
            record = inspect_compilation(root, target, "cross", "default", "a" * 40, {}, "1.99.1")
            self.assertEqual(record["artifact"]["features"], "default")
            for name, changed in (("build.stdout", '{"reason":"build-finished","success":true}\n'),
                                  ("metadata.stdout", '{"packages":[]}')):
                with self.subTest(name=name):
                    path = root / name
                    original = path.read_bytes()
                    path.write_text(banner + changed, encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, "captured compiler stdout"):
                        inspect_compilation(root, target, "cross", "default", "a" * 40, {}, "1.99.1")
                    path.unlink()
                    with self.assertRaisesRegex(ValueError, "compiler stdout"):
                        inspect_compilation(root, target, "cross", "default", "a" * 40, {}, "1.99.1")
                    path.write_bytes(original)

    def test_cross_decoder_does_not_discard_unknown_output_or_native_errors(self):
        from verification.product_build import compiler_stdout
        banner = "  1.99.1-x86_64-unknown-linux-gnu unchanged - rustc 1.99.1 (58cf8f9c3 2026-10-13)\n"
        for prefix in ("unrecognized launcher text\n", banner.replace("1.99.1", "1.99.0"),
                       banner.replace("unchanged", "failed"), banner):
            output = prefix + '{}\n' if prefix != banner else prefix
            with self.subTest(output=output), self.assertRaises(ValueError):
                compiler_stdout(output, "cross", "1.99.1")
        native = banner + '{}\n'
        self.assertEqual(compiler_stdout(native, "cargo", "1.99.1"), native)
        with self.assertRaises(json.JSONDecodeError):
            json.loads(compiler_stdout(native, "cargo", "1.99.1"))
        late = '{}\n' + banner
        with self.assertRaises(json.JSONDecodeError):
            json.loads(compiler_stdout(late, "cross", "1.99.1"))


class ProductBuild(unittest.TestCase):
    def test_fixed_release_builder_retains_actual_default_and_telemetry_units(self):
        from verification.product_build import compile_product, inspect_compilation
        target, channel = native_compiler()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root)
            commit_source(root)
            for features in ("default", "telemetry"):
                output = root / ".git" / features
                record = compile_product(root, target, "cargo", output, dict(os.environ), features)
                result = inspect_compilation(output, target, "cargo", features,
                                             record["source"]["revision"], record["source"]["stamped"], channel)
                self.assertEqual(result, record)
                self.assertTrue(record["command"][-1].startswith("--message-format=json"))
                self.assertEqual(record["artifact"]["sha256"], sha256(root / "target" / target / "release" /
                                 ("empack.exe" if os.name == "nt" else "empack")))
                before = {p.name: p.read_bytes() for p in output.iterdir()}
                with self.assertRaisesRegex(ValueError, "already contains an attempt"):
                    compile_product(root, target, "cargo", output, dict(os.environ), features)
                self.assertEqual(before, {p.name: p.read_bytes() for p in output.iterdir()})

    def test_fixed_release_builder_refuses_transitive_fixture_authority(self):
        from verification.product_build import compile_product
        target, _ = native_compiler()
        for leak in ("feature", "package"):
            with self.subTest(leak=leak), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                fixture(root, leak)
                commit_source(root)
                output = root / ".git/compiled"
                with self.assertRaisesRegex(ValueError, "Test-only"):
                    compile_product(root, target, "cargo", output, dict(os.environ))
                self.assertFalse((output / "compile.json").exists())

    def test_archive_record_copies_the_actual_stamped_compilation_without_rebuild(self):
        import tarfile
        import zipfile
        from verification.product_build import inspect_compilation, COMPILATION_FILES
        from verification.release import archive_name
        from verification.release_inputs import prepare
        target, channel = native_compiler()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root)
            with (root / "Cargo.toml").open("a") as stream:
                stream.write('[workspace.metadata.empack]\nrelease-target = "0.8.4-beta"\n')
            commit_source(root)
            inputs = prepare(root, "0.8.4-beta")
            inputs_path = root / ".git/inputs.json"
            inputs_path.write_text(json.dumps(inputs))
            compiled = root / ".git/compiled"
            environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
            producer = Path(__file__).parents[1] / "release-artifacts.py"
            command = subprocess.run([sys.executable, str(producer), "build", "--target", target,
                                      "--builder", "cargo", "--output", str(compiled)], cwd=root,
                                     env=environment, capture_output=True, text=True, timeout=120)
            self.assertEqual(command.returncode, 0, command.stderr)
            binary = root / "target" / target / "release" / ("empack.exe" if os.name == "nt" else "empack")
            before = binary.read_bytes()
            archive = root / ".git" / archive_name("0.8.4-beta", target)
            if archive.suffix == ".zip":
                with zipfile.ZipFile(archive, "w") as writer:
                    writer.write(binary, binary.name)
            else:
                with tarfile.open(archive, "w:gz") as writer:
                    writer.add(binary, arcname=binary.name)
            output = root / ".git/recorded"
            command = subprocess.run([sys.executable, str(producer), "record", "--inputs", str(inputs_path),
                                      "--archive", str(archive), "--target", target, "--builder", "cargo",
                                      "--output", str(output), "--compilation", str(compiled)], cwd=root,
                                     env=environment, capture_output=True, text=True, timeout=120)
            self.assertEqual(command.returncode, 0, command.stderr)
            record = json.loads((output / "build.json").read_text())
            self.assertEqual((output / archive.name).read_bytes(), archive.read_bytes())
            self.assertEqual(binary.read_bytes(), before)
            self.assertTrue(COMPILATION_FILES <= {p.name for p in output.iterdir()})
            actual = inspect_compilation(output, target, "cargo", "default", inputs["source_revision"],
                                         inputs["stamped"], channel)
            self.assertEqual(actual["artifact"]["sha256"], record["executable"]["sha256"])

    def test_real_default_telemetry_and_cached_artifacts_are_audited(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root)
            for name, features in (("default", "default"), ("cached", "default"), ("telemetry", "telemetry")):
                artifact = build(root, name, features)
                audit = inspect_retained_build(root / name, artifact)
                self.assertEqual(audit["executable_sha256"], sha256(artifact["path"]))
                self.assertNotIn("empack-tests", {u["workspace_package"] for u in audit["units"]})
                library = next(u for u in audit["units"] if u["workspace_package"] == "empack-lib")
                self.assertEqual("telemetry" in library["features"], features == "telemetry")
                if name == "cached":
                    messages = [json.loads(line) for line in (root / name / "product-build-messages.jsonl").read_text().splitlines()]
                    self.assertTrue(all(e["fresh"] for e in messages if e["reason"] == "compiler-artifact"))

    def test_real_transitive_test_feature_and_package_leakage_are_rejected(self):
        for leak in ("feature", "package"):
            with self.subTest(leak=leak), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                fixture(root, leak)
                with self.assertRaisesRegex(ValueError, "Test-only"):
                    build(root)
                self.assertFalse((root / "attempt/product-version.log").exists(), "refuse before product execution")

    def test_missing_failed_truncated_or_inconsistent_build_records_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root)
            artifact = build(root)
            attempt = root / "attempt"
            events = [json.loads(line) for line in (attempt / "product-build-messages.jsonl").read_text().splitlines()]
            metadata = json.loads((attempt / "product-metadata.json").read_text())
            mutations = []
            mutations.append(events[:-1])
            mutations.append(events + [events[-1]])
            for change in ("failed", "test", "features", "executable", "library"):
                altered = copy.deepcopy(events)
                binary = next(e for e in altered if e.get("executable") == artifact["path"])
                if change == "failed":
                    altered[-1]["success"] = False
                elif change == "test":
                    binary["profile"]["test"] = True
                elif change == "features":
                    binary["features"] = ["telemetry"]
                elif change == "executable":
                    binary["executable"] += ".other"
                else:
                    altered = [e for e in altered if e.get("target", {}).get("name") != "empack_lib"]
                mutations.append(altered)
            for changed in mutations:
                with self.subTest(changed=changed[-1]), self.assertRaises(ValueError):
                    inspect_build("\n".join(map(json.dumps, changed)), metadata, "default", artifact["path"], artifact["sha256"])
            with self.assertRaises(ValueError):
                inspect_build('{"reason":', metadata, "default", artifact["path"], artifact["sha256"])

    def test_retained_audit_is_recomputed_and_does_not_need_original_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root)
            artifact = build(root)
            attempt = root / "attempt"
            Path(artifact["path"]).unlink()  # Admission also runs on a different machine.
            with patch("pathlib.Path.resolve", side_effect=AssertionError("producer filesystem unavailable")):
                inspect_retained_build(attempt, artifact)
            audit = attempt / "product-build-audit.json"
            document = json.loads(audit.read_text())
            document["units"] = []
            audit.write_text(json.dumps(document))
            artifact["build_audit_sha256"] = sha256(audit)
            with self.assertRaisesRegex(ValueError, "differs"):
                inspect_retained_build(attempt, artifact)

    def test_suite_admission_requires_and_rechecks_runner_build_evidence(self):
        import shutil
        import test_ci_verification as ci
        from verification.evidence import write_json
        from verification.receipts import inspect_suite_receipt
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root)
            artifact = build(root)
            target = "aarch64-apple-darwin"
            artifact["target"] = target  # Synthetic suite, not target execution evidence.
            attempt, receipt = ci.fixture_attempt(root / "suite", target, "default", ["cli"])
            receipt["artifact"] = artifact
            for name in ("product-build-messages.jsonl", "product-metadata.json", "product-build-audit.json"):
                shutil.copyfile(root / "attempt" / name, attempt / name)
            path = attempt / "receipt.json"
            write_json(path, receipt)
            inspect_suite_receipt(path, ["cli"], target, ci.REVISION, "default")
            del artifact["build_audit_sha256"]
            write_json(path, receipt)
            with self.assertRaisesRegex(ValueError, "audit differs"):
                inspect_suite_receipt(path, ["cli"], target, ci.REVISION, "default")
            (attempt / "product-build-messages.jsonl").unlink()
            with self.assertRaisesRegex(ValueError, "Missing"):
                inspect_suite_receipt(path, ["cli"], target, ci.REVISION, "default")
