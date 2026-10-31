"""Synthetic build/evidence admission; these tests do not compile telemetry."""
import copy
import json
from pathlib import Path
import shutil
import subprocess
import sys
import unittest
from unittest.mock import patch

import test_ci_verification as ci
import test_release_gate as archives
from verification.auxiliary import SUITES, build_command, inspect_auxiliary
from verification.evidence import sha256, write_json
from verification.qualification import qualify
from compiled_product_fixture import compilation


class AuxiliaryBuild(unittest.TestCase):
    git = archives.ReleaseArchiveGate.git
    setUp = archives.ReleaseArchiveGate.setUp
    admit = archives.ReleaseArchiveGate.admit

    def test_workflow_upload_keeps_compilation_and_suite_evidence_for_admission(self):
        workflow = (ci.SCRIPTS.parent / ".github/workflows/release.yml").read_text()
        auxiliary = workflow.split("  auxiliary:", 1)[1].split("  candidate-gate:", 1)[0]
        upload = auxiliary.split("name: candidate-auxiliary-", 1)[1]
        patterns = upload.split("path: |", 1)[1].split("include-hidden-files:", 1)[0]
        parent = self.root / ".git/auxiliary"
        parent.mkdir()
        self.fixture("auxiliary/one-build")
        downloaded = self.root / ".git/downloaded"
        for pattern in patterns.splitlines():
            if not pattern.strip():
                continue
            for source in self.root.glob(pattern.strip()):
                if source.is_file():
                    destination = downloaded / source.relative_to(parent)
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(source, destination)
        result = inspect_auxiliary(downloaded / "one-build/build.json", self.root, self.revision, self.version)
        self.assertEqual(result["suite"]["status"], "PASS")
        self.assertFalse(result["shipped"])

    def fixture(self, name="auxiliary"):
        bundle = self.root / ".git" / name
        bundle.mkdir()
        executable = bundle / "empack"
        executable.write_bytes(b"synthetic telemetry executable; never run")
        target = "aarch64-apple-darwin"
        with patch.object(ci, "REVISION", self.revision):
            attempt, receipt = ci.fixture_attempt(bundle / "suite", target, "telemetry", SUITES)
        receipt["artifact"].update(version=self.version, profile="release", sha256=sha256(executable))
        write_json(attempt / "receipt.json", receipt)
        record = dict(schema=1, kind="candidate-auxiliary-build", source_revision=self.revision,
                      version=self.version, target=target, features="telemetry", profile="release", shipped=False,
                      created_at="2026-10-23T00:00:00+00:00", status="PASS", inputs=self.inputs,
                      command=build_command(target), compiler=f"rustc fixture\nrelease: 1.99.1\nhost: {target}",
                      build_exit_code=0, build_timed_out=False,
                      build_environment=dict(GIT_HASH=self.revision, BUILD_DATE=self.inputs["build_date"], CARGO_INCREMENTAL="0"),
                      executable=dict(sha256=sha256(executable), length=executable.stat().st_size),
                      compilation_sha256=compilation(bundle, target, sha256(executable), self.revision,
                                                     self.inputs["stamped"], features="telemetry"),
                      suite_receipt_sha256=sha256(attempt / "receipt.json"))
        write_json(bundle / "build.json", record)
        return bundle / "build.json", record, attempt / "receipt.json"

    def test_bound_telemetry_enters_candidate_without_becoming_shipped_package_evidence(self):
        self.admit()
        path, _, _ = self.fixture()
        output = self.root / ".git/candidate.json"
        process = subprocess.run([sys.executable, str(ci.SCRIPTS / "qualify-release.py"), "gate",
            "--payload", str(self.output), "--archive-evidence-sha256", sha256(self.output / "archive-evidence.json"),
            "--auxiliary", str(path), "--revision", self.revision, "--version", self.version, "--output", str(output)],
            cwd=self.root, capture_output=True, text=True, timeout=30)
        self.assertEqual(process.returncode, 2, process.stderr)
        record = json.loads(output.read_text())
        self.assertFalse(record["qualification"])
        self.assertEqual(len(record["suite_evidence"]), 6)
        self.assertEqual(record["suite_evidence"][-1]["features"], "telemetry")
        self.assertFalse(record["auxiliary_builds"][0]["shipped"])
        self.assertEqual(record["auxiliary_builds"][0]["build_sha256"], sha256(path))
        package = next(c for r in record["acceptance"]["requirements"] for c in r["cases"] if c["id"] == "UX-085.packaged-local-export-prepare")
        self.assertEqual(len(package["variants"]), 5)
        self.assertTrue(all(v["features"] == "default" for v in package["variants"]))

    def test_mismatched_build_inputs_features_compiler_command_or_bytes_are_rejected(self):
        path, original, _ = self.fixture()
        changes = [("source_revision", "f" * 40), ("version", "0.8.3-beta"), ("features", "default"),
                   ("shipped", True), ("profile", "dev"), ("build_exit_code", 1), ("build_timed_out", True),
                   ("command", build_command(original["target"]) + ["--all-features"]),
                   ("compiler", original["compiler"].replace("1.99.1", "1.98.0")),
                   ("build_environment", dict(original["build_environment"], RUSTFLAGS="--cfg fixture"))]
        for key, value in changes:
            changed = dict(original, **{key: value})
            write_json(path, changed)
            with self.subTest(field=key), self.assertRaises(ValueError):
                inspect_auxiliary(path, self.root, self.revision, self.version)
        changed = copy.deepcopy(original)
        changed["inputs"]["stamped"]["Cargo.lock"] = "0" * 64
        write_json(path, changed)
        with self.assertRaisesRegex(ValueError, "Build inputs"):
            inspect_auxiliary(path, self.root, self.revision, self.version)
        write_json(path, original)
        (path.parent / "empack").write_bytes(b"replacement bytes")
        with self.assertRaisesRegex(ValueError, "executable differs"):
            inspect_auxiliary(path, self.root, self.revision, self.version)

    def test_suite_cannot_relabel_binary_or_hide_an_earlier_failed_attempt(self):
        self.admit()
        path, record, receipt_path = self.fixture()
        receipt = json.loads(receipt_path.read_text())
        original = copy.deepcopy(receipt)
        receipt["artifact"]["sha256"] = "d" * 64
        write_json(receipt_path, receipt)
        record["suite_receipt_sha256"] = sha256(receipt_path)
        write_json(path, record)
        with self.assertRaisesRegex(ValueError, "built executable"):
            inspect_auxiliary(path, self.root, self.revision, self.version)
        original["status"] = "FAIL"
        original["suites"][0].update(status="FAIL", exit_code=1)
        write_json(receipt_path, original)
        record.update(status="FAIL", suite_receipt_sha256=sha256(receipt_path))
        write_json(path, record)
        passed, _, _ = self.fixture("later-pass")
        result = qualify(self.root, self.output, sha256(self.output / "archive-evidence.json"),
                         self.revision, self.version, auxiliary_paths=[path, passed])
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual([r["status"] for r in result["suite_evidence"][-2:]], ["FAIL", "PASS"])

    def test_missing_duplicate_and_modified_suite_evidence_cannot_qualify(self):
        path, record, receipt = self.fixture()
        original = receipt.read_bytes()
        receipt.unlink()
        with self.assertRaisesRegex(ValueError, "one contained"):
            inspect_auxiliary(path, self.root, self.revision, self.version)
        receipt.write_bytes(original + b" ")
        with self.assertRaisesRegex(ValueError, "changed after"):
            inspect_auxiliary(path, self.root, self.revision, self.version)
        receipt.write_bytes(original)
        extra = path.parent / "suite/extra/receipt.json"
        extra.parent.mkdir()
        write_json(extra, {})
        with self.assertRaisesRegex(ValueError, "one contained"):
            inspect_auxiliary(path, self.root, self.revision, self.version)
        extra.unlink()
        (receipt.parent / "cli/junit.xml").write_text("unbound replacement")
        with self.assertRaisesRegex(ValueError, "Changed test evidence"):
            inspect_auxiliary(path, self.root, self.revision, self.version)

    def test_auxiliary_recomputes_the_compiled_graph_even_when_hashes_are_updated(self):
        path, record, _ = self.fixture()
        compile_path = path.parent / "compile.json"
        compiled = json.loads(compile_path.read_text())
        audit_path = path.parent / "product-build-audit.json"
        audit = json.loads(audit_path.read_text())
        audit["units"] = []
        write_json(audit_path, audit)
        compiled["artifact"]["build_audit_sha256"] = sha256(audit_path)
        write_json(compile_path, compiled)
        record["compilation_sha256"] = sha256(compile_path)
        write_json(path, record)
        with self.assertRaisesRegex(ValueError, "audit differs"):
            inspect_auxiliary(path, self.root, self.revision, self.version)


if __name__ == "__main__":
    unittest.main()
