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
from verification.evidence import selected_tests, sha256, write_json
from verification.suites import SUITES, nextest_command
from consumer_fixture import prism_record
from test_python_evidence import fixture as python_fixture
from test_doctest_evidence import fixture as doctest_fixture
from test_authoring_schema import fixture as schema_fixture
from verification.consumer import inspect_prism_record

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
            ("crates/empack-lib/src/application/engine_host/cli.rs", ["live-provider-cli", "live-modrinth"], False),
            ("crates/empack-lib/src/application/session.rs", ["live-provider-cli", "live-modrinth"], False),
            ("crates/empack-lib/src/engine/providers/curseforge.rs", ["live-provider-cli", "live-modrinth", "live-curseforge"], False),
            ("crates/empack-lib/src/engine/providers/selector.rs", ["live-provider-cli", "live-modrinth", "live-curseforge"], False),
            ("crates/empack-tests/tests/live_provider_cli.rs", ["live-provider-cli", "live-modrinth", "live-curseforge"], False),
            ("crates/empack-tests/tests/live_provider_cli/migration.rs", ["live-provider-cli", "live-modrinth"], False),
            ("crates/empack-lib/tests/live_provider_contract.rs", ["live-provider-cli", "live-modrinth", "live-curseforge"], False),
            ("crates/empack-lib/tests/live_runtime_catalog_contract.rs", ["live-catalog", "server-cli", "runtime-current"], False),
            ("crates/empack-lib/tests/live_server_distribution_startup.rs", ["live-catalog", "server-cli", "runtime-current"], False),
            ("crates/empack-lib/src/engine/build/server/unix.rs", ["live-catalog", "server-cli", "runtime-current", "pinned-import"], False),
            ("crates/empack-tests/tests/cli_server.rs", ["live-catalog", "server-cli", "runtime-current"], False),
            ("crates/empack-tests/src/bin/java-fixture.rs", ["live-catalog", "server-cli", "runtime-current"], False),
            ("crates/empack-lib/src/application/engine_host/instance/prelaunch.rs", ["live-catalog", "server-cli", "runtime-current"], False),
            ("crates/empack-lib/src/engine/prism.rs", ["pinned-import"], True),
            ("crates/empack-lib/src/engine/build/client/fixtures.json", ["pinned-import"], True),
            ("tests/fixtures/import-corpus/fabulously-optimized-6.5.0.json", ["live-provider-cli", "live-modrinth", "pinned-import"], False),
            ("crates/empack-tests/tests/live_pinned_import.rs", ["pinned-import"], False),
            (".gitattributes", ["publisher-cli"], False),
            ("crates/empack-lib/src/engine/api/release_publication/channel.rs", ["publisher-cli"], False),
            ("crates/empack-lib/src/engine/release/trust.rs", ["publisher-cli"], False),
            ("crates/empack-lib/src/application/engine_host/release.rs", ["publisher-cli"], False),
            ("crates/empack-tests/tests/cli_publisher.rs", ["publisher-cli"], False),
            ("crates/empack-tests/tests/support/publisher.rs", ["publisher-cli"], False),
            ("tests/fixtures/hosted-publication/manifest.json", ["publisher-cli"], False),
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
        self.assertIn("publisher-cli", plan["live_suites"])
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


def tooling_attempt(directory):
    schema_fixture(directory / "verification-schema" / "one-attempt")
    classification = json.loads((ci.ROOT / "tests/verification/python-bindings.json").read_text())
    records = []
    for target in ci.TARGETS:
        name = "verification-python" if target == ci.TARGETS[0] else f"verification-python-{target}"
        records.append(python_fixture(ci.ROOT, directory / name / "one-attempt", classification, target))
    return records[0]


def native_attempts(directory):
    """Synthetic native matrix; no compiler, executable or consumer is invoked."""
    tooling_attempt(directory)
    for target in ci.TARGETS:
        for features, suites in ci.NATIVE_SUITES.items():
            fixture_attempt(directory / f"verification-{target}" / features, target, features, suites,
                            ci.NATIVE_TEST_FEATURES[features])
            graph = ci.NATIVE_TEST_FEATURES[features]
            doctest_fixture(directory / f"verification-{target}" / f"doctest-{graph}" / "attempt", target, graph)


def fixture_attempt(directory, target, features="telemetry", suites=None, test_features="all"):
    suites = suites or ["deterministic", "telemetry"]
    attempt = directory / "one-attempt"
    attempt.mkdir(parents=True)
    write_json(attempt / "source-snapshot.json", {
        "fixture": "unit-test, not real acceptance",
        "tests/verification/requirements.json": {"sha256": sha256(ci.REQUIREMENTS), "executable": False},
    })
    receipt = dict(schema=1, kind="test-suite-run", status="PASS", source_revision=REVISION, worktree_dirty=False,
                   created_at="2026-10-23T00:00:00+00:00",
                   source_snapshot_sha256=sha256(attempt / "source-snapshot.json"),
                   requested_suites=suites, suites=[], test_features=test_features,
                   artifact=dict(target=target, features=features, profile="dev", sha256="c" * 64))
    for name in suites:
        if reason := SUITES[name].inapplicable_reason(target):
            receipt["suites"].append(dict(suite=name, status="NOT_APPLICABLE", reason=reason))
            continue
        suite_dir = attempt / name
        suite_dir.mkdir()
        policy = json.loads(ci.CLASSIFICATION.read_text())
        platform = policy["targets"][target]
        rows = [row for row in policy["tests"] if platform in row["platforms"] and test_features in row["test_features"]
                and row["binary_name"] not in SUITES[name].excluded_binaries
                and (not SUITES[name].binaries or row["binary_name"] in SUITES[name].binaries)]
        inventory = {"test-count": len(rows), "rust-build-meta": {"platforms": {
            "host": {"platform": {"triple": target}}, "targets": []}}, "rust-suites": {}}
        for row in rows:
            binary = row["binary"]
            test = row["id"].removeprefix(binary + "::")
            suite = inventory["rust-suites"].setdefault(binary, {
                "status": "listed", "binary-name": row["binary_name"], "testcases": {}})
            matches = not SUITES[name].cases or test in SUITES[name].cases
            suite["testcases"][test] = {"ignored": row["ignored"],
                                       "filter-match": {"status": "matches" if matches else "mismatch"}}
        write_json(suite_dir / "inventory.json", inventory)
        identities, _ = selected_tests(inventory, SUITES[name].ignored, SUITES[name].binaries, SUITES[name].cases,
                                        SUITES[name].excluded_binaries)
        # Synthetic successful rows test gate mechanics; these are not executions
        # of the real test IDs used to construct the expected receipt shape.
        xml = []
        for binary, suite in inventory["rust-suites"].items():
            cases = ''.join(f'<testcase classname="{binary}" name="{test}"/>'
                            for test in suite["testcases"] if f'{binary}::{test}' in identities)
            xml.append(f'<testsuite name="{binary}">{cases}</testsuite>')
        (suite_dir / "junit.xml").write_text(f'<testsuites tests="{len(identities)}">' + ''.join(xml) + '</testsuites>')
        receipt["suites"].append(dict(suite=name, status="PASS", exit_code=0, timed_out=False, attempt=1,
            selected=identities, tests=[dict(id=identity, status="PASS") for identity in identities],
            classification_sha256=sha256(ci.CLASSIFICATION),
            discovery_command=nextest_command("list", SUITES[name], suite_dir / "nextest.toml", test_features),
            command=nextest_command("run", SUITES[name], suite_dir / "nextest.toml", test_features),
            inventory_sha256=sha256(suite_dir / "inventory.json"), junit_sha256=sha256(suite_dir / "junit.xml")))
    write_json(attempt / "receipt.json", receipt)
    return attempt, receipt


class RequiredGate(unittest.TestCase):
    def test_schema_observations_are_required_even_for_docs_only_changes(self):
        plan = bound(ci.selection(["docs/schemas/empack.schema-5.json"], trusted=False))
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            tooling_attempt(root)
            result = ci.check_gate(plan, successes(plan), root)
            self.assertEqual(result["schema_evidence"][0]["status"], "PASS")
            row = next(row for row in result["acceptance"]["requirements"] if row["id"] == "UX-006")
            case = next(case for case in row["cases"] if case["id"] == "UX-006.editor-schema-cli-conformance")
            self.assertEqual(case["status"], "PASS")
            from verification.reporting import render_gate
            self.assertIn("Editor schema and CLI conformance", render_gate(result))
            path = root / "verification-schema/one-attempt/receipt.json"
            record = json.loads(path.read_text())
            path.unlink()
            with self.assertRaisesRegex(ValueError, "schema conformance receipt"):
                ci.check_gate(plan, successes(plan), root)
            write_json(path, dict(record, observations_sha256="e" * 64))
            with self.assertRaisesRegex(ValueError, "changed schema observations"):
                ci.check_gate(plan, successes(plan), root)
            write_json(path, record)
            other = root / "verification-schema/duplicate/receipt.json"
            other.parent.mkdir()
            write_json(other, record)
            with self.assertRaisesRegex(ValueError, "exactly one schema"):
                ci.check_gate(plan, successes(plan), root)

    def test_missing_failed_duplicate_or_relabelled_native_python_cannot_satisfy_gate(self):
        plan = bound(ci.selection(["crates/empack-tests/tests/cli_entry.rs"], trusted=True))
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            native_attempts(root)
            result = ci.check_gate(plan, successes(plan), root)
            self.assertEqual({r["compiler_host"] for r in result["tooling_evidence"]}, set(ci.TARGETS))
            for target in ci.TARGETS[1:]:
                path = root / f"verification-python-{target}/one-attempt/receipt.json"
                original = path.read_bytes()
                path.unlink()
                with self.assertRaisesRegex(ValueError, "one Python attempt"):
                    ci.check_gate(plan, successes(plan), root)
                path.write_bytes(original)
                data = json.loads(original)
                data.update(status="FAIL", exit_code=1)
                write_json(path, data)
                with self.assertRaisesRegex(ValueError, "Native Python execution failed"):
                    ci.check_gate(plan, successes(plan), root)
                data = json.loads(original)
                data["source_revision"] = "f" * 40
                write_json(path, data)
                with self.assertRaisesRegex(ValueError, "Unrelated"):
                    ci.check_gate(plan, successes(plan), root)
                path.write_bytes(original)
                duplicate = path.parent.parent / "duplicate/receipt.json"
                duplicate.parent.mkdir()
                duplicate.write_bytes(original)
                with self.assertRaisesRegex(ValueError, "one Python attempt"):
                    ci.check_gate(plan, successes(plan), root)
                duplicate.unlink()
            path = root / "verification-python-aarch64-apple-darwin/one-attempt/receipt.json"
            data = json.loads(path.read_text())
            data["compiler_tools"]["rustc"] = data["compiler_tools"]["rustc"].replace("aarch64-apple-darwin", "x86_64-apple-darwin")
            write_json(path, data)
            with self.assertRaisesRegex(ValueError, "another host"):
                ci.check_gate(plan, successes(plan), root)

    def test_evidence_inputs_are_stable_in_autocrlf_checkout(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def git(*args):
                subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)
            git("init", "-q")
            git("config", "core.autocrlf", "true")
            git("config", "core.safecrlf", "false")
            attributes = SCRIPTS.parent / ".gitattributes"
            (root / ".gitattributes").write_bytes(attributes.read_bytes() if attributes.exists() else b"")
            from verification.python_tests import inputs
            from verification.doctests import inputs as doctest_inputs
            paths = {"tests/verification/test-bindings.json", *inputs(SCRIPTS.parent),
                     *doctest_inputs(SCRIPTS.parent)}
            originals = {}
            for name in paths:
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                originals[name] = (SCRIPTS.parent / name).read_bytes().replace(b"\r\n", b"\n")
                target.write_bytes(originals[name])
            # Prove conversion really runs, without extending evidence rules to
            # ordinary author files or changing exact signed-fixture bytes.
            author = root / "author/note.txt"
            signed = root / "tests/fixtures/hosted-publication/opaque.bin"
            for path, content in [(author, b"author note\n"), (signed, b"signed\r\n\0opaque\n")]:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(content)
            git("add", ".")
            for path in [*(root / name for name in paths), author, signed]:
                path.unlink()
            git("checkout-index", "--all", "--force")
            for name, original in originals.items():
                self.assertEqual((root / name).read_bytes(), original,
                                 f"Evidence input bytes changed under native checkout conversion: {name}")
            self.assertEqual(author.read_bytes(), b"author note\r\n")
            self.assertEqual(signed.read_bytes(), b"signed\r\n\0opaque\n")

    def test_consumer_source_dependency_and_fixture_identity_are_required(self):
        record = prism_record(REVISION)
        inspect_prism_record(record, REVISION)
        for mutate in (
            lambda r: r.update(qt_version="5.15.2"),
            lambda r: r["environment"].update(qt_runtime="6.5.0"),
            lambda r: r.update(prism_commit="d" * 40),
            lambda r: r.update(empack_dirty=True),
            lambda r: r.update(fixture_sha256="d" * 64),
            lambda r: r["fixtures"].pop(),
            lambda r: r["fixtures"][0]["consumer"].update(override=False),
        ):
            modified = copy.deepcopy(record)
            mutate(modified)
            with self.assertRaises(ValueError):
                inspect_prism_record(modified, REVISION)

    def test_consumer_job_observations_reach_cases_and_the_report(self):
        from verification.reporting import render_gate
        plan = bound(ci.selection(["scripts/check-prism-config.py"], trusted=True))
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            native_attempts(root)
            path = root / "consumer-prism/prism.json"
            path.parent.mkdir()
            record = prism_record(REVISION)
            write_json(path, record)
            result = ci.check_gate(plan, successes(plan), root)
            row = next(r for r in result["acceptance"]["requirements"] if r["id"] == "UX-053")
            self.assertEqual(row["cases"][0]["status"], "PASS")
            self.assertEqual(row["status"], "NOT_RUN")
            self.assertIn("Consumer parser observations", render_gate(result))
            self.assertIn("consumer probe:", render_gate(result))
            self.assertEqual(result["consumer_evidence"][0]["receipt_sha256"], sha256(path))
            record["fixtures"][0]["consumer"]["prepare"] = "missing integration"
            record["status"] = "FAIL"
            write_json(path, record)
            with self.assertRaisesRegex(ValueError, "observations failed"):
                ci.check_gate(plan, successes(plan), root)

    def test_docs_gate_allows_deliberately_unselected_jobs(self):
        plan = bound(ci.selection(["docs/testing.md"], trusted=False))
        needs = successes(plan)
        needs["test"] = {"result": "skipped"}
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            tooling_attempt(root)
            result = ci.check_gate(plan, needs, root)
            self.assertEqual(result["status"], "PASS")
            self.assertEqual(result["tooling_evidence"][0]["features"], "independent")

    def test_green_tooling_job_requires_observed_python_results(self):
        plan = bound(ci.selection(["docs/testing.md"], trusted=False))
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with self.assertRaisesRegex(ValueError, "Python execution receipt"):
                ci.check_gate(plan, successes(plan), root)
            record = tooling_attempt(root)
            path = root / "verification-python/one-attempt/receipt.json"
            record["timed_out"] = True
            record["status"] = "FAIL"
            write_json(path, record)
            with self.assertRaisesRegex(ValueError, "did not pass"):
                ci.check_gate(plan, successes(plan), root)

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
            native_attempts(root)
            result = ci.check_gate(plan, successes(plan), root)
            self.assertEqual(len(result["suite_evidence"]), 6)
            self.assertEqual(len(result["doctest_evidence"]), 6)
            self.assertEqual({r["compiler_host"] for r in result["tooling_evidence"]}, set(ci.TARGETS))
            mac = root / "verification-aarch64-apple-darwin/default/one-attempt/receipt.json"
            original = json.loads(mac.read_text())
            forged = copy.deepcopy(original)
            row = next(r for r in forged["suites"] if r["suite"] == "native-remount")
            row.clear()
            row.update(suite="native-remount", status="NOT_APPLICABLE", reason="permission denied")
            write_json(mac, forged)
            with self.assertRaisesRegex(ValueError, "applicability"):
                ci.check_gate(plan, successes(plan), root)
            write_json(mac, original)
            missing = copy.deepcopy(original)
            missing["suites"] = [r for r in missing["suites"] if r["suite"] != "native-remount"]
            write_json(mac, missing)
            with self.assertRaises(ValueError):
                ci.check_gate(plan, successes(plan), root)
            write_json(mac, original)
            doc = root / f"verification-{ci.TARGETS[0]}" / "doctest-all" / "attempt" / "receipt.json"
            original_doc = json.loads(doc.read_text())
            doc.unlink()
            with self.assertRaisesRegex(ValueError, "rustdoc attempt"):
                ci.check_gate(plan, successes(plan), root)
            write_json(doc, {**original_doc, "status": "FAIL", "exit_code": 101})
            with self.assertRaisesRegex(ValueError, "rustdoc execution failed"):
                ci.check_gate(plan, successes(plan), root)
            write_json(doc, original_doc)
            self.assertFalse(result["qualification"])
            # Full receipts are admitted first. The join must retain every native
            # variant without presenting partial requirement decomposition as done.
            acceptance = result["acceptance"]["requirements"]
            self.assertEqual(len(acceptance), 109)
            declared = {case["id"]: {(platform, features, graph)
                        for platform in case.get("required_targets", case["required_platforms"])
                        for features in case["required_product_features"]
                        for graph in case.get("required_test_features", [None])}
                        for row in json.loads(ci.REQUIREMENTS.read_text())["requirements"]
                        for case in row["cases"]}
            native_complete = {
                "UX-002", "UX-005", "UX-006", "UX-007", "UX-011", "UX-022", "UX-023",
                "UX-024", "UX-027", "UX-028", "UX-029", "UX-036", "UX-038",
                "UX-040", "UX-051", "UX-052", "UX-054", "UX-057", "UX-058", "UX-059", "UX-063", "UX-065", "UX-066", "UX-075",
            }
            for requirement in acceptance:
                # This synthetic full native matrix now supplies every bound
                # configuration, status, explanation, replacement, content-kind,
                # side-choice/definition changes, pending, build and signing-key setup cases.
                # Entry identity needs package proof; partial groups cannot pass merely because their current tests do.
                self.assertEqual(requirement["status"],
                                 "PASS" if requirement["id"] in native_complete else "NOT_RUN",
                                 requirement["id"])
                for case in requirement["cases"]:
                    # This plan selects native suites, not the independently bound
                    # live-provider case. Only selected execution can pass it.
                    missing = [test for variant in case["variants"] for test in variant["tests"]
                               if not test["receipts"]]
                    self.assertEqual(case["status"], "NOT_RUN" if missing else "PASS")
                    if case["boundary"] == "P":
                        # Complete definitions do not turn native development
                        # executions into evidence for the five final archives.
                        self.assertEqual(case["status"], "NOT_RUN")
                        self.assertEqual(len(case["variants"]), 5)
                        self.assertTrue(all(not test["receipts"] for variant in case["variants"]
                                            for test in variant["tests"]))
                    if case["id"] == "UX-026.live-pinned-update":
                        self.assertEqual(case["status"], "NOT_RUN")
                    actual = [(v.get("target", v["platform"]), v["features"], v.get("test_features")) for v in case["variants"]]
                    self.assertEqual(set(actual), declared[case["id"]])
                    self.assertEqual(len(actual), len(set(actual)))
            partial_evidence = [
                ci.check_suite_receipt(root / f"verification-{target}" / features,
                                       suites, target, REVISION, features,
                                       test_features=ci.NATIVE_TEST_FEATURES[features])
                for target in ci.TARGETS for features, suites in ci.NATIVE_SUITES.items()
                if not (target == "x86_64-pc-windows-msvc" and features == "default")
            ]
            partial = ci.case_results(
                json.loads(ci.REQUIREMENTS.read_text()),
                json.loads(ci.CLASSIFICATION.read_text()),
                partial_evidence, REVISION,
                schema_evidence=result["schema_evidence"],
            )
            key_setup = next(r for r in partial["requirements"] if r["id"] == "UX-054")
            self.assertEqual(key_setup["status"], "NOT_RUN")
            enrollment = next(r for r in partial["requirements"] if r["id"] == "UX-057")
            self.assertEqual(enrollment["status"], "NOT_RUN")
            channel_auth = next(r for r in partial["requirements"] if r["id"] == "UX-058")
            self.assertEqual(channel_auth["status"], "NOT_RUN")
            rotation = next(r for r in partial["requirements"] if r["id"] == "UX-059")
            self.assertEqual(rotation["status"], "NOT_RUN")
            inspection = next(r for r in partial["requirements"] if r["id"] == "UX-063")
            self.assertEqual(inspection["status"], "NOT_RUN")
            choices = next(r for r in partial["requirements"] if r["id"] == "UX-066")
            self.assertEqual(choices["status"], "NOT_RUN")
            authoring = next(r for r in partial["requirements"] if r["id"] == "UX-006")
            self.assertEqual(authoring["status"], "NOT_RUN")
            retirement = next(r for r in partial["requirements"] if r["id"] == "UX-075")
            self.assertEqual(retirement["status"], "NOT_RUN")
            conflicts = next(r for r in partial["requirements"] if r["id"] == "UX-065")
            self.assertEqual(conflicts["status"], "NOT_RUN")
            windows = next(c for c in key_setup["cases"]
                           if c["id"] == "UX-054.windows-private-directory-refusal")
            self.assertEqual({(v["features"], v["status"]) for v in windows["variants"]},
                             {("default", "NOT_RUN"), ("telemetry", "PASS")})
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
                lambda r: r["artifact"].update(profile="release"),
                lambda r: r["suites"][0].update(status="BLOCKED"),
                lambda r: r["suites"][0].update(tests=[]),
                lambda r: r["suites"][0].update(attempt=2),
                lambda r: r["suites"][0].update(classification_sha256="e" * 64),
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

    def test_minimal_graph_cannot_be_replaced_by_relabelled_all_features_commands(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            attempt, receipt = fixture_attempt(root, ci.TARGETS[0], "default", ["deterministic"], "minimal")
            result = ci.check_suite_receipt(root, ["deterministic"], ci.TARGETS[0], REVISION,
                                            "default", test_features="minimal")
            self.assertEqual(result["test_features"], "minimal")
            for key in ("discovery_command", "command"):
                modified = copy.deepcopy(receipt)
                command = modified["suites"][0][key]
                command[command.index("--no-default-features")] = "--all-features"
                write_json(attempt / "receipt.json", modified)
                with self.subTest(key=key), self.assertRaisesRegex(ValueError, "feature graph/selection"):
                    ci.check_suite_receipt(root, ["deterministic"], ci.TARGETS[0], REVISION,
                                           "default", test_features="minimal")
            receipt["test_features"] = "all"
            write_json(attempt / "receipt.json", receipt)
            with self.assertRaisesRegex(ValueError, "Wrong workspace test feature graph"):
                ci.check_suite_receipt(root, ["deterministic"], ci.TARGETS[0], REVISION,
                                       "default", test_features="minimal")

    def test_inventory_host_cannot_be_relabelled_by_artifact_provenance(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            attempt, receipt = fixture_attempt(root, ci.TARGETS[0], suites=["telemetry"])
            inventory_path = attempt / "telemetry" / "inventory.json"
            inventory = json.loads(inventory_path.read_text())
            # Telemetry has the same identities on both hosts. Its compiler host
            # still cannot be relabelled by the caller's artifact target field.
            inventory["rust-build-meta"]["platforms"]["host"]["platform"]["triple"] = ci.TARGETS[1]
            write_json(inventory_path, inventory)
            receipt["suites"][0]["inventory_sha256"] = sha256(inventory_path)
            write_json(attempt / "receipt.json", receipt)
            with self.assertRaisesRegex(ValueError, "Inventory target disagrees"):
                ci.check_suite_receipt(root, ["telemetry"], ci.TARGETS[0], REVISION)

    def test_selected_case_missing_from_discovery_cannot_be_substituted(self):
        from verification.evidence import selected_tests
        with tempfile.TemporaryDirectory() as temp:
            attempt, _ = fixture_attempt(Path(temp), ci.TARGETS[0])
            inventory = json.loads((attempt / "deterministic" / "inventory.json").read_text())
            with self.assertRaisesRegex(ValueError, "Empty required selection"):
                selected_tests(inventory, False, ("empack-core",), ("another_fixture",))

    def test_missing_credentials_are_blocked_before_discovery_or_execution(self):
        from verify import Blocked, run_suite
        sentinel = "live_cli_curseforge_selectors_converge"
        self.assertIn(sentinel, SUITES["live-curseforge"].cases)
        self.assertTrue(SUITES["live-provider-cli"].cases)
        self.assertIn("live_cli_runtime_migration_preserves_pins_and_verifies_consumer_outputs", SUITES["live-provider-cli"].cases)
        self.assertNotIn(sentinel, SUITES["live-provider-cli"].cases)
        for name in ("live-provider", "live-curseforge", "live-runtime"):
            with self.subTest(name=name), self.assertRaisesRegex(Blocked, "prerequisites unavailable"):
                run_suite(name, SUITES[name], Path("unused"), {}, {"features": "telemetry", "target": ci.TARGETS[0]}, {}, "all")


class PushPlanning(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "-q")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        (self.root / "README.md").write_text("original\n")
        self.commit("initial")
        self.base = self.git("rev-parse", "HEAD")

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True).strip()

    def commit(self, message):
        self.git("add", ".")
        self.git("commit", "-qm", message)

    def plan(self, **overrides):
        event = dict(before=self.base, after=self.git("rev-parse", "HEAD"),
                     ref="refs/heads/main", deleted=False,
                     repository={"full_name": "fixture/upstream"})
        event.update(overrides)
        event_path = self.root / ".git/event.json"
        event_path.write_text(json.dumps(event))
        result = subprocess.run([sys.executable, str(SCRIPTS / "ci-verification.py"),
            "plan", "--output", str(self.root / ".git/plan.json")], cwd=self.root,
            capture_output=True, text=True, env=dict(os.environ, GITHUB_EVENT_PATH=str(event_path),
                GITHUB_EVENT_NAME="push", GITHUB_REPOSITORY="fixture/upstream",
                GITHUB_OUTPUT=str(self.root / ".git/outputs")))
        if result.returncode:
            return result, None
        return result, json.loads((self.root / ".git/plan.json").read_text())

    def test_docs_push_gets_tooling_and_a_terminal_gate(self):
        (self.root / "README.md").write_text("changed\n")
        self.commit("docs")
        result, plan = self.plan()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(plan["required_jobs"], ["tooling"])
        self.assertEqual(plan["changed_paths"], ["README.md"])
        self.assertEqual(plan["source_revision"], self.git("rev-parse", "HEAD"))
        self.assertTrue(plan["trusted"])

    def test_removed_code_in_a_push_still_requires_the_native_matrix(self):
        (self.root / "source.rs").write_text("source\n")
        self.commit("code")
        self.base = self.git("rev-parse", "HEAD")
        self.git("mv", "source.rs", "source.md")
        self.commit("rename")
        result, plan = self.plan(ref="refs/heads/dev")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(plan["changed_paths"], ["source.md", "source.rs"])
        self.assertEqual(plan["native_targets"], list(ci.TARGETS))

    def test_new_branch_selects_all_paths_without_inventing_a_prior_tree(self):
        (self.root / "Cargo.lock").write_text("fixture dependency inputs\n")
        self.commit("new dev snapshot")
        result, plan = self.plan(before="0" * 40, ref="refs/heads/dev")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(plan["changed_paths"], ["Cargo.lock", "README.md"])
        self.assertIn("live", plan["required_jobs"])
        self.assertTrue(plan["consumer"])

    def test_checkout_mismatch_and_untrusted_or_deleted_refs_fail_closed(self):
        for values in (dict(after="f" * 40), dict(before="not-a-revision"),
                       dict(ref="refs/heads/unreviewed"), dict(deleted=True),
                       dict(repository={"full_name": "fixture/fork"})):
            with self.subTest(values=values):
                result, plan = self.plan(**values)
                self.assertNotEqual(result.returncode, 0)
                self.assertIsNone(plan)

    def test_unavailable_before_revision_cannot_become_docs_only_success(self):
        result, plan = self.plan(before="f" * 40)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(plan)


if __name__ == "__main__":
    unittest.main()
