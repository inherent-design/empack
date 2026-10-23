"""Synthetic join tests; these records are not product acceptance evidence."""
import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from verification.requirements import case_results, validate_requirements

REVISION = "a" * 40


class AcceptanceResults(unittest.TestCase):
    def setUp(self):
        self.case = {"id": "UX-001.identity", "boundary": "X",
                     "test_ids": ["cli::version", "cli::help"],
                     "required_platforms": ["linux", "windows"],
                     "required_product_features": ["default", "telemetry"]}
        self.row = {"id": "UX-001", "scope": "stable", "risk": "critical",
                    "evidence_tiers": ["X", "P"], "case_bindings_status": "complete",
                    "cases": [self.case]}
        self.document = {"schema": 1, "requirements": [self.row]}
        self.bindings = {"targets": {"linux-target": "linux", "windows-target": "windows"},
                         "tests": [{"id": identity, "boundary": "X", "platforms": ["linux", "windows"], "test_features": ["minimal", "all"]}
                                   for identity in self.case["test_ids"]]}
        self.evidence = [{"source_revision": REVISION, "receipt_sha256": str(index) * 64,
                          "artifact_sha256": str(index + 4) * 64, "target": platform + "-target",
                          "features": features, "test_features": "minimal" if features == "default" else "all", "tests": [{"id": identity, "status": "PASS"}
                            for identity in self.case["test_ids"]]}
                         for index, (platform, features) in enumerate(
                             (p, f) for p in ("linux", "windows") for f in ("default", "telemetry"))]

    def result(self):
        return case_results(self.document, self.bindings, self.evidence, REVISION)

    def test_each_required_variant_retains_its_exact_tests_artifact_and_receipt(self):
        report = self.result()
        self.assertFalse(report["qualification"])
        self.assertEqual(report["requirements"][0]["status"], "PASS")
        variants = report["requirements"][0]["cases"][0]["variants"]
        self.assertEqual(len(variants), 4)
        for variant, evidence in zip(variants, self.evidence, strict=True):
            self.assertEqual(variant["status"], "PASS")
            self.assertEqual([test["id"] for test in variant["tests"]], self.case["test_ids"])
            for test in variant["tests"]:
                self.assertEqual(test["artifact_sha256"], evidence["artifact_sha256"])
                self.assertEqual(test["test_features"], evidence["test_features"])
                self.assertEqual(test["receipts"], [evidence["receipt_sha256"]])

    def test_missing_test_host_or_product_feature_is_not_a_pass(self):
        original = copy.deepcopy(self.evidence)
        for missing in ("test", "host", "feature"):
            self.evidence = copy.deepcopy(original)
            if missing == "test":
                self.evidence[0]["tests"].pop()
            elif missing == "host":
                self.evidence = [row for row in self.evidence if row["target"] != "windows-target"]
            else:
                self.evidence = [row for row in self.evidence if row["features"] != "telemetry"]
            with self.subTest(missing=missing):
                report = self.result()["requirements"][0]
                self.assertEqual(report["status"], "NOT_RUN")
                self.assertEqual(report["cases"][0]["status"], "NOT_RUN")

    def test_partial_or_empty_decomposition_cannot_qualify_a_requirement(self):
        self.row["case_bindings_status"] = "partial"
        self.assertEqual(self.result()["requirements"][0]["cases"][0]["status"], "PASS")
        self.assertEqual(self.result()["requirements"][0]["status"], "NOT_RUN")
        self.row.update(case_bindings_status="pending", cases=[])
        self.assertEqual(self.result()["requirements"][0]["status"], "NOT_RUN")

    def test_failure_and_blocked_outcomes_survive_incomplete_decomposition(self):
        self.row["case_bindings_status"] = "partial"
        for status in ("FAIL", "BLOCKED", "NOT_RUN"):
            self.evidence[0]["tests"][0]["status"] = status
            with self.subTest(status=status):
                self.assertEqual(self.result()["requirements"][0]["status"], status)

    def test_receipt_cannot_substitute_another_revision_boundary_or_platform(self):
        self.bindings["tests"][0]["test_features"] = ["all"]
        with self.assertRaisesRegex(ValueError, "unclassified test variant"):
            self.result()
        self.bindings["tests"][0]["test_features"] = ["minimal", "all"]
        self.evidence[0]["source_revision"] = "b" * 40
        with self.assertRaisesRegex(ValueError, "another revision"):
            self.result()
        self.evidence[0]["source_revision"] = REVISION
        self.case["boundary"] = "P"
        with self.assertRaisesRegex(ValueError, "required test boundary"):
            self.result()
        self.case["boundary"] = "X"
        self.bindings["tests"][0]["platforms"] = ["linux"]
        self.evidence = []
        with self.assertRaisesRegex(ValueError, "unclassified platform"):
            self.result()

    def test_case_variants_and_concrete_membership_must_be_unambiguous(self):
        for key, value in (("required_platforms", ["linuz"]), ("required_platforms", ["linux", "linux"]),
                           ("required_product_features", ["default", "default"]),
                           ("required_product_features", ["unknown"]), ("test_ids", ["not-discovered"])):
            original = copy.deepcopy(self.case[key])
            self.case[key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                self.result()
            self.case[key] = original
        validate_requirements(self.document)

    def test_repeated_evidence_does_not_multiply_cases_or_erase_a_first_failure(self):
        duplicate = copy.deepcopy(self.evidence[0])
        duplicate["receipt_sha256"] = "f" * 64
        self.evidence.append(duplicate)
        variant = self.result()["requirements"][0]["cases"][0]["variants"][0]
        self.assertEqual(len(variant["tests"]), 2)
        self.assertEqual(len(variant["tests"][0]["receipts"]), 2)
        duplicate["tests"][0]["status"] = "FAIL"
        with self.assertRaisesRegex(ValueError, "Conflicting"):
            self.result()
        duplicate["tests"][0]["status"] = "PASS"
        duplicate["artifact_sha256"] = "e" * 64
        with self.assertRaisesRegex(ValueError, "Conflicting"):
            self.result()


if __name__ == "__main__":
    unittest.main()
