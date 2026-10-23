"""Exercise change selection and final evidence admission, including skipped jobs."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("ci_verification", SCRIPTS / "ci-verification.py")
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)
from verification.evidence import sha256, write_json
from verification.suites import SUITES

REVISION = "a" * 40


class ChangeSelection(unittest.TestCase):
    def test_docs_and_editor_schemas_select_executable_tooling_without_native_matrix(self):
        plan = ci.selection(["README.md", ".github/README.md", "docs/schemas/empack.schema-5.json"], trusted=False)
        self.assertEqual(plan["required_jobs"], ["tooling"])
        self.assertFalse(plan["native_targets"])
        self.assertFalse(plan["blocked_jobs"])

    def test_unknown_non_document_changes_cannot_skip_native_checks(self):
        for path in ("new-component/data.bin", "docs/schemas/helper.py"):
            with self.subTest(path=path):
                plan = ci.selection([path], trusted=False)
                self.assertEqual(plan["native_targets"], list(ci.TARGETS))
                self.assertIn("test", plan["required_jobs"])
                self.assertFalse(plan["live_suites"])

    def test_provider_runtime_and_consumer_boundaries_select_distinct_evidence(self):
        cases = [
            ("crates/empack-lib/src/engine/providers/modrinth.rs", ["live-provider-cli", "live-modrinth"], False),
            ("crates/empack-lib/src/engine/providers/curseforge.rs", ["live-provider-cli", "live-modrinth", "live-curseforge"], False),
            ("crates/empack-lib/src/engine/build/server/unix.rs", ["live-catalog", "server-cli", "runtime-current"], False),
            ("crates/empack-lib/src/engine/build/client/fixtures.json", [], True),
        ]
        for path, suites, consumer in cases:
            with self.subTest(path=path):
                plan = ci.selection([path], trusted=True)
                self.assertEqual(plan["live_suites"], suites)
                self.assertEqual(plan["consumer"], consumer)
                self.assertFalse(plan["survey"])

    def test_untrusted_required_checks_remain_blocked_not_waived(self):
        plan = ci.selection(["Cargo.lock"], trusted=False)
        self.assertEqual(plan["blocked_jobs"], ["live", "consumer"])
        self.assertIn("live", plan["required_jobs"])
        with self.assertRaisesRegex(ValueError, "Trusted execution"):
            ci.check_gate(bound(plan), successes(plan), Path("not-needed"))

    def test_manual_full_run_keeps_credentialed_and_survey_obligations(self):
        plan = ci.selection([], trusted=True, full=True)
        self.assertIn("live-curseforge", plan["live_suites"])
        self.assertIn("survey", plan["required_jobs"])
        self.assertTrue(plan["consumer"])

    def test_actual_git_rename_from_code_to_docs_still_requires_native_checks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, text=True).strip()
            git("init", "-q")
            git("config", "user.name", "Fixture")
            git("config", "user.email", "fixture@example.invalid")
            (root / "code.rs").write_text("source\n")
            git("add", ".")
            git("commit", "-qm", "fixture")
            base = git("rev-parse", "HEAD")
            git("mv", "code.rs", "notes.md")
            git("commit", "-qm", "rename")
            head = git("rev-parse", "HEAD")
            event = {"pull_request": {"base": {"sha": base}, "head": {
                "sha": head, "repo": {"full_name": "fixture/fork"}}}}
            event_path = root / "event.json"
            event_path.write_text(json.dumps(event))
            output = root / "outputs"
            result = subprocess.run([sys.executable, str(SCRIPTS / "ci-verification.py"),
                "plan", "--output", str(root / "plan.json")], cwd=root, capture_output=True, text=True,
                env=dict(os.environ, GITHUB_EVENT_PATH=str(event_path), GITHUB_EVENT_NAME="pull_request",
                         GITHUB_REPOSITORY="fixture/upstream", GITHUB_OUTPUT=str(output)))
            self.assertEqual(result.returncode, 0, result.stderr)
            plan = json.loads((root / "plan.json").read_text())
            self.assertEqual(plan["changed_paths"], ["code.rs", "notes.md"])
            self.assertTrue(plan["native"])
            self.assertFalse(plan["trusted"])
            self.assertEqual(plan["source_revision"], head)
            self.assertIn("native=true", output.read_text())


def bound(plan):
    return dict(plan, source_revision=REVISION, source_tree="b" * 40)


def successes(plan):
    return {job: {"result": "success"} for job in ["plan", *plan["required_jobs"]]}


def fixture_attempt(directory, target, features="telemetry", suites=None):
    suites = suites or ["deterministic", "telemetry"]
    attempt = directory / "one-attempt"
    attempt.mkdir(parents=True)
    write_json(attempt / "source-snapshot.json", {"fixture": "unit-test, not real acceptance"})
    receipt = dict(schema=1, kind="test-suite-run", status="PASS", source_revision=REVISION, worktree_dirty=False,
                   source_snapshot_sha256=sha256(attempt / "source-snapshot.json"),
                   requested_suites=suites, suites=[],
                   artifact=dict(target=target, features=features, sha256="c" * 64))
    for name in suites:
        suite_dir = attempt / name
        suite_dir.mkdir()
        binaries = SUITES[name].binaries or ("empack-core",)
        identities = sorted(binary + "::fixture" for binary in binaries)
        inventory = {"test-count": len(binaries), "rust-suites": {binary: {
            "status": "listed", "binary-name": binary, "testcases": {
                "fixture": {"ignored": SUITES[name].ignored, "filter-match": {"status": "matches"}}}
            } for binary in binaries}}
        write_json(suite_dir / "inventory.json", inventory)
        rows = ''.join(f'<testsuite name="{binary}"><testcase classname="{binary}" name="fixture"/></testsuite>' for binary in binaries)
        (suite_dir / "junit.xml").write_text(f'<testsuites tests="{len(binaries)}">{rows}</testsuites>')
        receipt["suites"].append(dict(suite=name, status="PASS", exit_code=0, timed_out=False, attempt=1,
            selected=identities, tests=[dict(id=identity, status="PASS") for identity in identities],
            inventory_sha256=sha256(suite_dir / "inventory.json"), junit_sha256=sha256(suite_dir / "junit.xml")))
    write_json(attempt / "receipt.json", receipt)
    return attempt, receipt


class RequiredGate(unittest.TestCase):
    def test_consumer_source_dependency_and_fixture_identity_are_required(self):
        record = dict(empack_commit=REVISION, empack_dirty=False, prism_commit=ci.PRISM_COMMIT,
            evidence_scope="supported-dependency headless parser probe", fixture_sha256=sha256(ci.FIXTURES),
            qt_version="6.4.2", environment=dict(qt_compiled="6.4.2", qt_runtime="6.4.2"),
            fixtures=[dict(fixture=case["name"], supported=case["accepted"])
                      for case in json.loads(ci.FIXTURES.read_text())])
        ci.check_consumer(record, REVISION)
        for mutate in (
            lambda r: r.update(qt_version="5.15.2"),
            lambda r: r["environment"].update(qt_runtime="6.5.0"),
            lambda r: r.update(prism_commit="d" * 40),
            lambda r: r.update(empack_dirty=True),
            lambda r: r.update(fixture_sha256="d" * 64),
            lambda r: r["fixtures"].pop(),
        ):
            modified = copy.deepcopy(record)
            mutate(modified)
            with self.assertRaises(ValueError):
                ci.check_consumer(modified, REVISION)

    def test_docs_gate_allows_deliberately_unselected_jobs(self):
        plan = bound(ci.selection(["docs/testing.md"], trusted=False))
        needs = successes(plan)
        needs["test"] = {"result": "skipped"}
        self.assertEqual(ci.check_gate(plan, needs, Path("unused"))["status"], "PASS")

    def test_required_missing_failed_cancelled_and_skipped_are_not_passes(self):
        plan = bound(ci.selection(["README.md"], trusted=True))
        for status in (None, "failure", "cancelled", "skipped", "unknown"):
            with self.subTest(status=status):
                needs = successes(plan)
                if status is None:
                    del needs["tooling"]
                else:
                    needs["tooling"]["result"] = status
                with self.assertRaisesRegex(ValueError, "Required job tooling"):
                    ci.check_gate(plan, needs, Path("unused"))

    def test_mutated_selection_cannot_erase_a_required_job(self):
        plan = bound(ci.selection(["crates/empack/src/main.rs"], trusted=True))
        plan["required_jobs"].remove("test")
        with self.assertRaisesRegex(ValueError, "plan no longer matches"):
            ci.check_gate(plan, successes(plan), Path("unused"))

    def test_native_gate_joins_all_three_actual_record_shapes(self):
        plan = bound(ci.selection(["crates/empack/src/main.rs"], trusted=True))
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for target in ci.TARGETS:
                for features, suites in ci.NATIVE_SUITES.items():
                    fixture_attempt(root / f"verification-{target}" / features, target, features, suites)
            result = ci.check_gate(plan, successes(plan), root)
            self.assertEqual(len(result["suite_evidence"]), 6)
            self.assertFalse(result["qualification"])
            (root / f"verification-{ci.TARGETS[1]}" / "default" / "one-attempt" / "receipt.json").unlink()
            with self.assertRaisesRegex(ValueError, "exactly one attempt"):
                ci.check_gate(plan, successes(plan), root)

    def test_wrong_identity_partial_outcomes_and_changed_evidence_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            attempt, receipt = fixture_attempt(root, ci.TARGETS[0])
            for mutate in (
                lambda r: r.update(source_revision="d" * 40),
                lambda r: r.update(worktree_dirty=True),
                lambda r: r["artifact"].update(target=ci.TARGETS[1]),
                lambda r: r["artifact"].update(features="default"),
                lambda r: r["suites"][0].update(status="BLOCKED"),
                lambda r: r["suites"][0].update(tests=[]),
                lambda r: r["suites"][0].update(attempt=2),
                lambda r: r.update(requested_suites=["deterministic"]),
            ):
                modified = copy.deepcopy(receipt)
                mutate(modified)
                write_json(attempt / "receipt.json", modified)
                with self.assertRaises(ValueError):
                    ci.check_suite_receipt(root, ["deterministic", "telemetry"], ci.TARGETS[0], REVISION)
            write_json(attempt / "receipt.json", receipt)
            (attempt / "telemetry" / "junit.xml").write_text("changed")
            with self.assertRaisesRegex(ValueError, "Changed test evidence"):
                ci.check_suite_receipt(root, ["deterministic", "telemetry"], ci.TARGETS[0], REVISION)

    def test_selected_case_missing_from_discovery_cannot_be_substituted(self):
        from verification.evidence import selected_tests
        with tempfile.TemporaryDirectory() as temp:
            attempt, _ = fixture_attempt(Path(temp), ci.TARGETS[0])
            inventory = json.loads((attempt / "deterministic" / "inventory.json").read_text())
            with self.assertRaisesRegex(ValueError, "Empty required selection"):
                selected_tests(inventory, False, ("empack-core",), ("another_fixture",))

    def test_missing_credentials_are_blocked_before_discovery_or_execution(self):
        from verify import Blocked, run_suite
        for name in ("live-provider", "live-curseforge", "live-runtime"):
            with self.subTest(name=name), self.assertRaisesRegex(Blocked, "prerequisites unavailable"):
                run_suite(name, SUITES[name], Path("unused"), {}, {"features": "telemetry"}, {})


if __name__ == "__main__":
    unittest.main()
