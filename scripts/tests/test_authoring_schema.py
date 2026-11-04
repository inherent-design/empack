"""Synthetic schema observations exercise admission, not empack or jsonschema."""
import copy
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
from types import ModuleType, SimpleNamespace
import tempfile
import tomllib
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from verification.authoring_schema import (ROOT, TEST_ID, fixtures, input_hashes,
                                           inspect_schema_evidence, observation_results)
from verification.evidence import sha256, write_json
from verification.product_build import retain_build
from verification.requirements import case_results
from compiled_product_fixture import compilation

REVISION = "a" * 40


def observations():
    rows = []
    for case in fixtures():
        before = {name: {"kind": "directory", "mode": 0o700} for name in ("home", "project")}
        for name, document in (("empack.yml", case["author"]), ("empack.bindings.yml", case["bindings"])):
            if document is not None:
                before["project/" + name] = dict(kind="file", mode=0o600,
                    sha256=hashlib.sha256(json.dumps(document).encode()).hexdigest())
        rows.append(dict(id=case["id"], schema_valid=case["expected"], before=before, after=copy.deepcopy(before),
                         exit_code=0 if case["expected"] else 1, timed_out=False,
                         stdout='{"author_schema": 5}' if case["expected"] else "",
                         stderr="" if case["expected"] else "synthetic validation refusal"))
    return rows


def fixture(directory, revision=REVISION):
    """Retained synthetic build/observations; no external tool executes."""
    directory.mkdir(parents=True, exist_ok=True)
    target = "x86_64-unknown-linux-gnu"
    compilation(directory, target, "c" * 64, revision, {})
    messages = [json.loads(line) for line in (directory / "product-build-messages.jsonl").read_text().splitlines()]
    for event in messages:
        if event["reason"] == "compiler-artifact":
            event["profile"].update(debug_assertions=True, opt_level="0")
    emitted = "\n".join(map(json.dumps, messages))
    metadata = json.loads((directory / "product-metadata.json").read_text())
    artifact = retain_build(directory, emitted, metadata, "default", "/fixture/target/x86_64-unknown-linux-gnu/release/empack", "c" * 64)
    artifact.update(target=target, profile="dev", version=tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"],
                    provenance="built by this runner from the ordinary product package")
    raw = dict(schema=1, kind="authoring-schema-observations", inputs=input_hashes(), artifact_sha256=artifact["sha256"],
               dependencies={"jsonschema": "4.23.0"}, interpreter_sha256="d" * 64, python_version="3.14.0", fixtures=observations())
    write_json(directory / "observations.json", raw)
    pin = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    record = dict(schema=1, kind="authoring-schema-run", source_revision=revision, worktree_dirty=False, qualification=False,
                  inputs=input_hashes(), created_at=datetime.now(timezone.utc).isoformat(), platform="linux", artifact=artifact,
                  rustc=f"release: {pin}\nhost: {target}", exit_code=0, timed_out=False, status="PASS",
                  observations_sha256=sha256(directory / "observations.json"))
    write_json(directory / "receipt.json", record)
    return record, raw


class SchemaEvidence(unittest.TestCase):
    def test_release_schema_needs_exact_admitted_product_context(self):
        from verification.candidate_execution import inspect_schema_runs
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            attempt = directory / "authoring-schema/one-attempt"
            record, _ = fixture(attempt)
            record["artifact"].update(profile="release", provenance="caller-supplied target/features; digest and version checked")
            write_json(attempt / "receipt.json", record)
            expected = {key: record["artifact"][key] for key in ("target", "features", "profile", "version", "sha256")}
            with self.assertRaisesRegex(ValueError, "audited ordinary default build"):
                inspect_schema_evidence(attempt / "receipt.json", REVISION)
            self.assertEqual(inspect_schema_evidence(attempt / "receipt.json", REVISION, expected_artifact=expected)["status"], "PASS")
            for key, value in (("sha256", "e" * 64), ("target", "aarch64-apple-darwin"),
                               ("profile", "dev"), ("version", "wrong")):
                with self.subTest(key=key), self.assertRaises(ValueError):
                    inspect_schema_evidence(attempt / "receipt.json", REVISION, expected_artifact=dict(expected, **{key: value}))
            args = (directory, expected["target"], {"default": expected["sha256"]}, REVISION, expected["version"])
            self.assertEqual(inspect_schema_runs(*args)[0]["artifact_sha256"], expected["sha256"])
            duplicate = directory / "authoring-schema/second/receipt.json"
            duplicate.parent.mkdir()
            write_json(duplicate, record)
            with self.assertRaisesRegex(ValueError, "one contained schema"):
                inspect_schema_runs(*args)
            duplicate.unlink()
            with self.assertRaisesRegex(ValueError, "Unexpected schema attempt"):
                inspect_schema_runs(directory, "aarch64-apple-darwin", {}, REVISION, expected["version"])
            (attempt / "receipt.json").unlink()
            with self.assertRaisesRegex(ValueError, "one contained schema"):
                inspect_schema_runs(*args)

    def test_actual_probe_isolates_every_command_and_retains_mutation_and_timeout(self):
        # Only imports and subprocess execution are substituted; exercise the
        # producer's actual fixture writes, environment and snapshot collection.
        schema_module, references = ModuleType("jsonschema"), ModuleType("referencing")
        schema_module.Draft202012Validator = object
        references.Registry = references.Resource = object
        spec = importlib.util.spec_from_file_location("authoring_schema_probe", ROOT / "scripts/check-authoring-schema.py")
        module = importlib.util.module_from_spec(spec)
        with patch.dict(sys.modules, jsonschema=schema_module, referencing=references):
            spec.loader.exec_module(module)
        calls = []
        def command(arguments, **options):
            home = Path(options["cwd"])
            project = Path(arguments[arguments.index("--workdir") + 1])
            self.assertEqual(options["timeout"], 30)
            self.assertTrue(options["capture_output"])
            self.assertEqual(options["env"]["HOME"], str(home))
            self.assertNotIn("EMPACK_KEY_CURSEFORGE", options["env"])
            self.assertEqual(arguments[-3:], ["manifest", "check", "--json"])
            self.assertEqual(project.parent, home.parent)
            calls.append(arguments)
            if len(calls) == 1:
                (project.parent / "state").mkdir()
            if len(calls) == 2:
                raise module.subprocess.TimeoutExpired(arguments, 30, output=b"partial output")
            return SimpleNamespace(returncode=0, stdout='{"author_schema": 5}', stderr="")
        validator = SimpleNamespace(is_valid=lambda _: True)
        with patch.object(module.subprocess, "run", side_effect=command), \
                patch.dict("os.environ", {"EMPACK_KEY_CURSEFORGE": "not forwarded"}):
            result = module.observe(Path("/synthetic/empack"), (validator, validator))
        self.assertEqual(len(calls), 44)
        self.assertIn("state", result[0]["after"])
        self.assertNotIn("state", result[1]["before"])
        self.assertTrue(result[1]["timed_out"])
        self.assertEqual(result[1]["stdout"], "partial output")
        self.assertEqual([row["status"] for row in observation_results(result)][:2], ["FAIL", "FAIL"])

    def test_fixed_fixture_set_requires_real_validation_and_read_only_responses(self):
        rows = observations()
        self.assertEqual(len(rows), 44)
        self.assertTrue(all(row["status"] == "PASS" for row in observation_results(rows)))
        for mutate in (
            lambda r: r[0].update(schema_valid=False),
            lambda r: r[0].update(exit_code=1),
            lambda r: r[0].update(stdout="not JSON"),
            lambda r: r[0].update(stdout='{"author_schema": 4}'),
            lambda r: r[0].update(timed_out=True),
            lambda r: r[0]["after"].update({"state": {"kind": "directory", "mode": 0o700}}),
            lambda r: r[0]["after"]["project/empack.yml"].update(sha256="e" * 64),
            lambda r: r[0]["after"]["project/empack.yml"].update(mode=0o400),
            lambda r: r[12].update(exit_code=-9),
            lambda r: r[12].update(exit_code=130),
            lambda r: r[12].update(stderr=""),
        ):
            changed = copy.deepcopy(rows)
            mutate(changed)
            self.assertIn("FAIL", {row["status"] for row in observation_results(changed)})
        for changed in (rows[:-1], rows + rows[:1], list(reversed(rows))):
            with self.assertRaisesRegex(ValueError, "source fixtures"):
                observation_results(changed)
        changed = copy.deepcopy(rows)
        for snapshot in ("before", "after"):
            changed[0][snapshot]["project/empack.yml"]["sha256"] = "f" * 64
        with self.assertRaisesRegex(ValueError, "author inputs"):
            observation_results(changed)

    def test_product_source_dependency_and_raw_observations_are_bound(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            record, raw = fixture(directory)
            path = directory / "receipt.json"
            self.assertEqual(inspect_schema_evidence(path, REVISION)["status"], "PASS")
            for mutate in (
                lambda r: r.update(source_revision="b" * 40),
                lambda r: r.update(worktree_dirty=True),
                lambda r: r.update(reason="final source verification failed"),
                lambda r: r.update(created_at="2026-10-30T00:00:00"),
                lambda r: r.update(timed_out=True),
                lambda r: r.update(platform="windows"),
                lambda r: r["artifact"].update(features="telemetry"),
                lambda r: r["artifact"].update(sha256="e" * 64),
                lambda r: r["artifact"].update(version="wrong"),
                lambda r: r["inputs"].update({"Cargo.lock": "f" * 64}),
            ):
                changed = copy.deepcopy(record)
                mutate(changed)
                write_json(path, changed)
                with self.assertRaises(ValueError):
                    inspect_schema_evidence(path, REVISION)
            for mutate in (
                lambda r: r.update(artifact_sha256="e" * 64),
                lambda r: r["dependencies"].update(jsonschema="0.0.0"),
                lambda r: r["inputs"].update({"Cargo.lock": "f" * 64}),
                lambda r: r["fixtures"].pop(),
            ):
                changed = copy.deepcopy(raw)
                mutate(changed)
                write_json(directory / "observations.json", changed)
                write_json(path, dict(record, observations_sha256=sha256(directory / "observations.json")))
                with self.assertRaises(ValueError):
                    inspect_schema_evidence(path, REVISION)
            write_json(directory / "observations.json", raw)
            write_json(path, record)
            (directory / "product-build-messages.jsonl").write_text("{}")
            with self.assertRaises((ValueError, KeyError)):
                inspect_schema_evidence(path, REVISION)

    def test_observed_failure_cannot_be_relabelled_as_a_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            record, raw = fixture(directory)
            raw["fixtures"][0]["after"]["cache"] = {"kind": "directory", "mode": 0o700}
            write_json(directory / "observations.json", raw)
            record["observations_sha256"] = sha256(directory / "observations.json")
            write_json(directory / "receipt.json", record)
            with self.assertRaisesRegex(ValueError, "disagrees"):
                inspect_schema_evidence(directory / "receipt.json", REVISION)
            record.update(status="FAIL", exit_code=1)
            write_json(directory / "receipt.json", record)
            result = inspect_schema_evidence(directory / "receipt.json", REVISION)
            self.assertEqual(result["status"], "FAIL")
            self.assertEqual(result["fixtures"][0]["status"], "FAIL")

    def test_schema_evidence_joins_only_its_product_host_and_keeps_other_boundaries_unexecuted(self):
        document = json.loads((ROOT / "tests/verification/requirements.json").read_text())
        document["requirements"] = [row for row in document["requirements"] if row["id"] == "UX-006"]
        classification = json.loads((ROOT / "tests/verification/test-bindings.json").read_text())
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            fixture(path)
            record = inspect_schema_evidence(path / "receipt.json", REVISION)
            def joined(evidence):
                return case_results(document, classification, [], REVISION, schema_evidence=evidence)["requirements"][0]
            empty = joined([])
            self.assertTrue(all(case["status"] == "NOT_RUN" for case in empty["cases"]))
            result = joined([record])
            schema = next(case for case in result["cases"] if case["id"] == "UX-006.editor-schema-cli-conformance")
            self.assertEqual(schema["status"], "PASS")
            self.assertEqual(schema["variants"][0]["tests"][0]["id"], TEST_ID)
            self.assertEqual(result["status"], "NOT_RUN")
            self.assertTrue(all(case["status"] == "NOT_RUN" for case in result["cases"] if case is not schema))
            with self.assertRaisesRegex(ValueError, "Unrelated"):
                joined([dict(record, source_revision="b" * 40)])


if __name__ == "__main__":
    unittest.main()
