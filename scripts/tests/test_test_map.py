import importlib.util
import json
import copy
import sys
import tempfile
from unittest.mock import patch
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

spec = importlib.util.spec_from_file_location(
    "test_map", Path(__file__).resolve().parents[1] / "check-test-map.py")
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class RequirementBindings(unittest.TestCase):
    def setUp(self):
        self.document = json.loads((Path(__file__).resolve().parents[2] /
            "tests/verification/requirements.json").read_text())
        self.row = next(row for row in self.document["requirements"] if row["cases"])

    def test_partial_bindings_remain_structurally_valid(self):
        requirements, cases = checker.validate_requirements(self.document)
        self.assertIn(self.row["id"], requirements)
        self.assertIn(self.row["cases"][0]["id"], cases)

    def test_empty_cases_cannot_claim_completed_bindings(self):
        self.row["cases"] = []
        self.row["case_bindings_status"] = "complete"
        with self.assertRaisesRegex(ValueError, "Binding status"):
            checker.validate_requirements(self.document)

    def test_case_requires_positive_test_membership(self):
        self.row["cases"][0]["test_ids"] = []
        with self.assertRaisesRegex(ValueError, "concrete tests"):
            checker.validate_requirements(self.document)

    def test_case_cannot_substitute_another_host_boundary(self):
        self.row["cases"][0]["boundary"] = "W"
        with self.assertRaisesRegex(ValueError, "unrelated evidence"):
            checker.validate_requirements(self.document)


class TestDiscoveryBinding(unittest.TestCase):
    def setUp(self):
        self.inventory = {"test-count": 1, "rust-suites": {
            "engine": {"status": "listed", "testcases": {
                "tests::reject": {"ignored": True}}}}}
        self.bindings = {"tests": [{"id": "engine::tests::reject", "boundary": "U"}]}

    def test_ignored_tests_remain_visible(self):
        tests = checker.compare_inventory(self.inventory, self.bindings)
        self.assertTrue(tests["engine::tests::reject"]["ignored"])

    def test_unknown_test_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "unclassified_tests"):
            checker.compare_inventory(self.inventory, {"tests": []})

    def test_vanished_selector_is_an_error(self):
        self.bindings["tests"][0]["id"] = "engine::tests::typo"
        with self.assertRaisesRegex(ValueError, "missing_tests"):
            checker.compare_inventory(self.inventory, self.bindings)

    def test_missing_binary_is_not_an_empty_pass(self):
        self.inventory["rust-suites"]["engine"]["status"] = "skipped"
        with self.assertRaisesRegex(ValueError, "not listed"):
            checker.compare_inventory(self.inventory, self.bindings)

    def test_empty_inventory_and_empty_classification_are_not_a_pass(self):
        with self.assertRaisesRegex(ValueError, "nonempty"):
            checker.compare_inventory({"test-count": 0, "rust-suites": {}}, {"tests": []})

    def test_count_mismatch_fails(self):
        self.inventory["test-count"] = 2
        with self.assertRaisesRegex(ValueError, "count"):
            checker.compare_inventory(self.inventory, self.bindings)

    def test_unrecognized_boundary_fails(self):
        self.bindings["tests"][0]["boundary"] = "mock-browser"
        with self.assertRaisesRegex(ValueError, "boundary"):
            checker.compare_inventory(self.inventory, self.bindings)


class SurfaceBindings(unittest.TestCase):
    def setUp(self):
        self.row = {"id": "empack build::arg:delivery", "kind": "argument",
                    "values": ["references", "bundled"]}
        self.inventory = {"source": "clap-command-factory", "surfaces": [self.row]}
        self.bindings = {"surfaces": [{"id": self.row["id"],
            "requirements": ["UX-040"], "definition": dict(self.row)}]}

    def test_observed_surface_has_requirement_owner(self):
        result = checker.compare_surfaces(self.inventory, self.bindings, {"UX-040"})
        self.assertIn(self.row["id"], result)

    def test_added_option_value_requires_review(self):
        self.row["values"] = ["references", "bundled", "different"]
        with self.assertRaisesRegex(ValueError, "definition changed"):
            checker.compare_surfaces(self.inventory, self.bindings, {"UX-040"})

    def test_structural_changes_require_review_even_when_names_stay_the_same(self):
        for field, value in {
            "environment": "DIFFERENT_ENV", "arity": {"min": 2, "max": 2},
            "short_aliases": ["x"], "trailing_values": True, "action": "Append",
        }.items():
            with self.subTest(field=field):
                inventory = copy.deepcopy(self.inventory)
                inventory["surfaces"][0][field] = value
                with self.assertRaisesRegex(ValueError, "definition changed"):
                    checker.compare_surfaces(inventory, self.bindings, {"UX-040"})

    def test_new_surface_is_unclassified(self):
        self.inventory["surfaces"].append({"id": "empack new", "kind": "leaf"})
        with self.assertRaisesRegex(ValueError, "unclassified_surfaces"):
            checker.compare_surfaces(self.inventory, self.bindings, {"UX-040"})

    def test_typo_in_requirement_is_not_an_owner(self):
        with self.assertRaisesRegex(ValueError, "requirement owner"):
            checker.compare_surfaces(self.inventory, self.bindings, {"UX-041"})

    def test_python_identity_is_classified_without_a_rust_wrapper(self):
        inventory = {"source": "unittest-loader", "tests": [
            {"id": "test_fixture.Checks.test_failure", "ignored": True}]}
        bindings = {"tests": [{"id": "python::test_fixture.Checks.test_failure", "boundary": "U"}]}
        result = checker.compare_python(inventory, bindings)
        self.assertTrue(result[bindings["tests"][0]["id"]]["ignored"])


class ToolingOwnership(unittest.TestCase):
    def test_python_owner_must_be_nonempty_and_known(self):
        inventory = {"source": "unittest-loader", "tests": [
            {"id": "module.Checks.test_fixture", "ignored": False}]}
        row = {"id": "python::module.Checks.test_fixture", "boundary": "H"}
        for owners in (None, [], ["UX-typo"]):
            row["requirements"] = owners
            with self.subTest(owners=owners), self.assertRaisesRegex(ValueError, "requirement owner"):
                checker.compare_python(inventory, {"tests": [row]}, {"UX-088"})
        row["requirements"] = ["UX-088"]
        self.assertEqual(len(checker.compare_python(inventory, {"tests": [row]}, {"UX-088"})), 1)

    def test_failed_discovery_cannot_reuse_an_old_inventory(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            prior = output / "python-inventory.json"
            prior.write_text('{"source":"unittest-loader","tests":[]}')
            with patch.object(checker, "run", return_value=(1, "", False)):
                with self.assertRaisesRegex(ValueError, "discovery failed"):
                    checker.discover_tooling(output, {"UX-088"})


class DoctestDiscovery(unittest.TestCase):
    def test_listing_preserves_ids_and_empty_crate_groups(self):
        text = "crates/core.rs - Type (line 1): test\n1 test, 0 benchmarks\n0 tests, 0 benchmarks\n"
        self.assertEqual(checker.rustdoc_tests(text), {"crates/core.rs - Type (line 1)"})

    def test_incomplete_duplicate_and_unknown_listings_fail(self):
        entry = "crates/core.rs - Type (line 1): test\n"
        for text in (entry, entry + "2 tests, 0 benchmarks\n", entry * 2 + "2 tests, 0 benchmarks\n",
                     "test passed\n", "0 tests, 0 benchmarks\n", ""):
            with self.subTest(text=text), self.assertRaises(ValueError):
                checker.rustdoc_tests(text)
        self.assertEqual(checker.rustdoc_tests("0 tests, 0 benchmarks\n", allow_empty=True), set())

    def test_newly_ignored_or_unowned_doctest_needs_review(self):
        name = "crates/core.rs - Type (line 1)"
        inventory = {"source": "rustdoc-list", "tests": [{"id": name, "ignored": False}]}
        entry = {"id": "doctest::" + name, "boundary": "U", "ignored": False, "requirements": ["UX-086"]}
        bindings = {"tests": [entry]}
        self.assertEqual(len(checker.compare_doctests(inventory, bindings, {"UX-086"})), 1)
        inventory["tests"][0]["ignored"] = True
        with self.assertRaisesRegex(ValueError, "ignore policy"):
            checker.compare_doctests(inventory, bindings, {"UX-086"})
        with self.assertRaisesRegex(ValueError, "requirement owner"):
            checker.compare_doctests(inventory, bindings, {"UX-001"})
