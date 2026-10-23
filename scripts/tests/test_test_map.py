import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "test_map", Path(__file__).resolve().parents[1] / "check-test-map.py")
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


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
