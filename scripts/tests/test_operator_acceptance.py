"""Synthetic operator records exercise admission, never actual consumer acceptance."""
import copy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from verification.acceptance import (PROCEDURES, ROOT, inspect_acceptance_evidence,
                                     procedures)
from verification.evidence import sha256, write_json

REVISION = "a" * 40
VERSION = "0.8.4-beta"
PAYLOAD = "b" * 64
PRODUCT = "c" * 64
TARGET = "x86_64-unknown-linux-gnu"
IDENTITY = "acceptance::prism::reference-instance"


def fixture(directory, identity=IDENTITY):
    """Literal synthetic observations; do not use this helper as acceptance data."""
    proof = directory / "observation.txt"
    proof.write_text("Synthetic retained observation, not a consumer execution.\n", encoding="utf-8")
    definition = procedures()[identity]
    record = dict(schema=1, kind="operator-acceptance", source_revision=REVISION,
                  version=VERSION, archive_evidence_sha256=PAYLOAD, artifact_sha256=PRODUCT,
                  procedures_sha256=sha256(ROOT / PROCEDURES), procedure=identity,
                  target=TARGET, platform="linux", features="default",
                  started_at="2026-10-30T10:00:00+00:00", completed_at="2026-10-30T10:10:00+00:00",
                  operator=dict(id="synthetic fixture operator", role=definition["operator_roles"][0]),
                  subject=dict(project_revision="synthetic project revision", distribution_sha256="d" * 64,
                               expected_inventory_sha256="e" * 64),
                  environment=dict(os_version="fixture OS", architecture="x86_64", tool="fixture consumer",
                                   tool_version="synthetic", runtime="fixture dependency"),
                  observations=[dict(step=step, status="PASS", observed="Synthetic observed result",
                                     evidence=[proof.name]) for step in definition["steps"]],
                  files=[dict(path=proof.name, sha256=sha256(proof), bytes=proof.stat().st_size)], status="PASS")
    path = directory / "acceptance.json"
    write_json(path, record)
    return path, record


class OperatorAcceptance(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.path, self.record = fixture(Path(self.temporary.name))
        self.products = {(TARGET, "default"): PRODUCT}

    def inspect(self, record=None):
        if record is not None:
            write_json(self.path, record)
        return inspect_acceptance_evidence(self.path, REVISION, VERSION, PAYLOAD, self.products)

    def test_retained_observations_keep_operator_scope_and_exact_candidate_identity(self):
        result = self.inspect()
        self.assertEqual(result["status"], "PASS", "synthetic admission only")
        self.assertEqual(result["artifact_sha256"], PRODUCT)
        self.assertEqual(result["receipt_sha256"], sha256(self.path))
        self.assertEqual(result["operator"], self.record["operator"])
        self.assertEqual(result["observations"], self.record["observations"])
        self.assertEqual(result["evidence_scope"], "operator-reported observations with retained supporting files")
        self.assertNotIn("qualification", result)

    def test_source_payload_procedure_and_product_cannot_be_selected_by_the_record(self):
        for field, wrong in [("source_revision", "d" * 40), ("version", "0.8.3-beta"),
                             ("archive_evidence_sha256", "e" * 64), ("artifact_sha256", "f" * 64),
                             ("procedures_sha256", "0" * 64), ("procedure", "acceptance::invented"),
                             ("features", "telemetry"), ("target", "aarch64-apple-darwin"),
                             ("platform", "windows")]:
            record = copy.deepcopy(self.record)
            record[field] = wrong
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.inspect(record)
        self.products = {}
        record = dict(self.record, artifact_sha256=None)
        with self.assertRaises(ValueError):
            self.inspect(record)

    def test_unperformed_or_failed_steps_cannot_become_a_passing_receipt(self):
        for status in ("FAIL", "BLOCKED", "NOT_RUN"):
            record = copy.deepcopy(self.record)
            record["observations"][0]["status"] = status
            with self.subTest(status=status), self.assertRaisesRegex(ValueError, "status differs"):
                self.inspect(record)
            record["status"] = status
            self.assertEqual(self.inspect(record)["status"], status)
        record = copy.deepcopy(self.record)
        record["files"] = []
        for row in record["observations"]:
            row.update(status="NOT_RUN", observed="Not performed", evidence=[])
        record["status"] = "NOT_RUN"
        self.assertEqual(self.inspect(record)["status"], "NOT_RUN")

    def test_required_steps_and_supporting_evidence_are_not_replaceable_by_summary_labels(self):
        mutations = [lambda r: r["observations"].pop(),
                     lambda r: r["observations"].reverse(),
                     lambda r: r["observations"].append(r["observations"][0]),
                     lambda r: r["observations"][0].update(status="WAIVED"),
                     lambda r: r["observations"][0].update(observed=""),
                     lambda r: r["observations"][0].update(evidence=[]),
                     lambda r: r["observations"][0].update(evidence=["missing.txt"])]
        for mutate in mutations:
            record = copy.deepcopy(self.record)
            mutate(record)
            with self.subTest(mutation=mutate), self.assertRaises(ValueError):
                self.inspect(record)

    def test_supporting_file_identity_and_path_confinement_are_rechecked(self):
        for name in ("../elsewhere", "/absolute", "sub/../file", "sub//file", "./file", "a\\b", "C:file", self.path.name):
            record = copy.deepcopy(self.record)
            record["files"][0]["path"] = name
            with self.subTest(path=name), self.assertRaises(ValueError):
                self.inspect(record)
        record = copy.deepcopy(self.record)
        record["files"].append(record["files"][0])
        with self.assertRaisesRegex(ValueError, "repeated"):
            self.inspect(record)
        for field, value in (("bytes", True), ("bytes", 999), ("sha256", "f" * 64)):
            record = copy.deepcopy(self.record)
            record["files"][0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.inspect(record)
        (self.path.parent / "observation.txt").write_text("Changed\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "changed"):
            self.inspect(self.record)

    def test_attribution_environment_dates_and_bounded_json_are_required(self):
        mutations = [lambda r: r["operator"].update(id=""),
                     lambda r: r["operator"].update(role="unrecorded"),
                     lambda r: r["environment"].update(tool_version=""),
                     lambda r: r["subject"].update(distribution_sha256=None),
                     lambda r: r.update(started_at="2026-10-30T10:00:00"),
                     lambda r: r.update(completed_at="2026-10-29T00:00:00+00:00")]
        for mutate in mutations:
            record = copy.deepcopy(self.record)
            mutate(record)
            with self.subTest(mutation=mutate), self.assertRaises(ValueError):
                self.inspect(record)
        self.path.write_text('{"schema":1,"schema":1}', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            self.inspect()
        write_json(self.path, self.record)
        with patch("verification.acceptance.MAX_RECORD", 4), self.assertRaises(ValueError):
            self.inspect()
        with patch("verification.acceptance.MAX_FILE", 4), self.assertRaises(ValueError):
            self.inspect()
        with patch.object(Path, "is_symlink", return_value=True), self.assertRaises(ValueError):
            self.inspect()

    def test_join_retains_failed_attempts_and_missing_host_or_feature_variants(self):
        from verification.requirements import case_results
        case = dict(id="UX-084.fixture", boundary="C", test_ids=[IDENTITY],
                    required_platforms=["linux"], required_product_features=["default", "telemetry"])
        document = dict(schema=1, requirements=[dict(id="UX-084", scope="stable", risk="critical",
                        evidence_tiers=["C"], case_bindings_status="complete", cases=[case])])
        classification = dict(targets={TARGET: "linux"}, tests=[])
        def result(records):
            return case_results(document, classification, [], REVISION, acceptance_evidence=records)["requirements"][0]
        passed = self.inspect()
        self.assertEqual(result([])["status"], "NOT_RUN")
        self.assertEqual(result([passed])["status"], "NOT_RUN", "one feature cannot stand in for another")
        case["required_product_features"] = ["default"]
        self.assertEqual(result([passed])["status"], "PASS", "synthetic join only")
        record = copy.deepcopy(self.record)
        record["observations"][0]["status"] = record["status"] = "FAIL"
        failed = self.inspect(record)
        self.assertEqual(result([failed, passed])["status"], "FAIL")
        self.assertEqual(result([passed, failed])["status"], "FAIL")
        with self.assertRaisesRegex(ValueError, "Unrelated"):
            result([dict(passed, source_revision="e" * 40)])

    def test_candidate_gate_requires_actual_task_records_beyond_passing_native_cases(self):
        from test_candidate_qualification import CandidatePolicy
        from verification.qualification import evaluate_requirements
        fixture = CandidatePolicy()
        fixture.setUp()
        row = next(row for row in fixture.document["requirements"] if row["id"] == "UX-084")
        row.update(evidence_tiers=["C"], cases=[dict(id="UX-084.operator", boundary="C",
                   test_ids=[IDENTITY], required_platforms=["linux"], required_product_features=["default"])])
        def evaluate(records):
            return evaluate_requirements(fixture.document, fixture.classification, fixture.evidence,
                REVISION, VERSION, fixture.package_proofs, acceptance_evidence=records)
        self.assertEqual(evaluate([])["status"], "BLOCKED")
        passed = self.inspect()
        self.assertTrue(evaluate([passed])["qualification"], "synthetic gate mechanics, not actual tasks")
        failed = copy.deepcopy(self.record)
        failed["observations"][0]["status"] = failed["status"] = "FAIL"
        self.assertEqual(evaluate([self.inspect(failed), passed])["status"], "FAIL")


if __name__ == "__main__":
    unittest.main()
