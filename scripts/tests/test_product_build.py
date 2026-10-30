"""Actual tiny Cargo builds and malformed evidence; not empack package acceptance."""
import copy
import json
import os
from pathlib import Path
import subprocess
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


class ProductBuild(unittest.TestCase):
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
