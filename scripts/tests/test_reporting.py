"""Reports preserve admitted facts; synthetic records are not product evidence."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
from verification.reporting import render_gate


def fixture():
    test = {"id": "cli::observed", "status": "PASS", "receipts": ["1" * 64],
            "artifact_sha256": "2" * 64, "test_features": "minimal"}
    case = {"id": "UX-001.observed", "boundary": "X", "status": "NOT_RUN", "variants": [
        {"platform": "linux", "features": "default", "status": "PASS", "tests": [test]},
        {"platform": "windows", "features": "default", "status": "NOT_RUN", "tests": [
            {"id": "cli::observed", "status": "NOT_RUN", "receipts": []}]}]}
    row = {"id": "UX-001", "scope": "stable", "risk": "critical", "case_bindings_status": "partial",
           "status": "NOT_RUN", "cases": [case]}
    return {"schema": 1, "status": "PASS", "qualification": False, "source_revision": "a" * 40,
            "source_tree": "b" * 40, "created_at": "2026-10-23T00:00:00+00:00",
            "acceptance": {"source_revision": "a" * 40, "qualification": False,
                           "requirements_sha256": "3" * 64, "classification_sha256": "4" * 64,
                           "requirements": [row]},
            "suite_evidence": [{"target": "linux-target", "features": "default", "test_features": "minimal",
                "profile": "dev", "suites": ["cli"], "artifact_sha256": "2" * 64,
                "receipt_sha256": "1" * 64, "source_snapshot_sha256": "5" * 64}]}


class VerificationReports(unittest.TestCase):
    def test_ci_success_does_not_qualify_partial_requirement_or_missing_variant(self):
        text = render_gate(fixture())
        self.assertIn("CI gate: **PASS**", text)
        self.assertIn("qualification has **not** been established", text)
        self.assertIn("| 0 | 0 | 0 | 1 |", text)
        self.assertIn("<code>partial</code> | <code>NOT_RUN</code>", text)
        self.assertIn("<code>windows</code> | <code>any classified target</code> | <code>default</code> | <code>NOT_RUN</code>", text)
        self.assertIn("No admitted execution for this test and variant", text)
        for identity in ("a" * 40, "b" * 40, *(str(n) * 64 for n in range(1, 6)), "cli::observed"):
            self.assertIn(identity, text)
        self.assertNotIn("%", text)

    def test_failed_admission_cannot_render_fabricated_passing_acceptance(self):
        record = fixture()
        record.update(status="FAIL", reason="Required job test is skipped")
        text = render_gate(record)
        self.assertIn("Required job test is skipped", text)
        self.assertIn("No complete acceptance join was admitted", text)
        self.assertNotIn("cli::observed", text)
        self.assertNotIn("## Admitted suite receipts", text)

    def test_unknown_qualification_waivers_or_mixed_sources_are_not_summarized_as_success(self):
        original = fixture()
        for change in (
            lambda r: r.update(qualification=True),
            lambda r: r.update(status="unknown"),
            lambda r: r["acceptance"].update(source_revision="c" * 40),
            lambda r: r["acceptance"]["requirements"][0].update(status="WAIVED"),
        ):
            record = copy.deepcopy(original)
            change(record)
            with self.assertRaises(ValueError):
                render_gate(record)

    def test_diagnostic_text_cannot_inject_html_or_table_structure(self):
        record = fixture()
        record.update(status="FAIL", reason='<script>unexpected</script> | value\nnext line')
        text = render_gate(record)
        self.assertNotIn("<script>", text)
        self.assertIn("&lt;script&gt;unexpected&lt;/script&gt; &#124; value<br>next line", text)

    def test_gate_subprocess_writes_success_and_failure_reports_with_actual_source(self):
        root = SCRIPTS.parent
        def git(*args):
            return subprocess.check_output(["git", *args], cwd=root, text=True).strip()
        revision, tree = git("rev-parse", "HEAD"), git("rev-parse", "HEAD^{tree}")
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp)
            plans = output / "verification-plan"
            plans.mkdir()
            # Literal docs-only plan: the subprocess still derives and checks its selection.
            plan = {"schema": 1, "qualification": False, "changed_paths": ["README.md"],
                    "trusted": True, "native": False, "live_suites": [], "consumer": False,
                    "survey": False, "required_jobs": ["tooling"], "native_targets": [],
                    "blocked_jobs": [], "source_revision": revision, "source_tree": tree}
            for mode, expected in (("success", "PASS"), ("skipped", "FAIL"),
                                   ("wrong-tree", "FAIL"), ("missing-plan", "FAIL")):
                with self.subTest(mode=mode):
                    candidate = dict(plan, source_tree="0" * 40) if mode == "wrong-tree" else plan
                    (plans / "plan.json").write_text(json.dumps(candidate))
                    if mode == "missing-plan":
                        (plans / "plan.json").unlink()
                    job_status = "skipped" if mode == "skipped" else "success"
                    env = dict(os.environ, CI_NEEDS_JSON=json.dumps({"plan": {"result": "success"},
                                                               "tooling": {"result": job_status}}))
                    run = subprocess.run([sys.executable, str(SCRIPTS / "ci-verification.py"), "gate",
                        "--artifacts", str(output), "--output", str(output / "gate.json")],
                        cwd=root, env=env, capture_output=True, text=True, timeout=20)
                    self.assertEqual(run.returncode == 0, expected == "PASS", run.stderr)
                    record = json.loads((output / "gate.json").read_text())
                    self.assertEqual(record["status"], expected)
                    self.assertEqual(record["source_revision"], revision)
                    self.assertEqual(record["source_tree"], tree)
                    self.assertTrue(record["created_at"])
                    text = (output / "gate.md").read_text()
                    self.assertIn(f"CI gate: **{expected}**", text)
                    self.assertIn(revision, text)
                    if expected == "PASS":
                        self.assertEqual(len(record["acceptance"]["requirements"]), 109)
                        self.assertTrue(all(row["status"] == "NOT_RUN" for row in record["acceptance"]["requirements"]))
                    elif mode == "skipped":
                        self.assertIn("Required job tooling", text)
                    elif mode == "wrong-tree":
                        self.assertIn("Gate checkout differs", text)
                    else:
                        self.assertIn("plan.json", text)


if __name__ == "__main__":
    unittest.main()
