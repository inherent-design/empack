import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "python_discovery", Path(__file__).resolve().parents[1] / "discover-python-tests.py")
discovery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(discovery)


class PythonDiscovery(unittest.TestCase):
    def test_loader_lists_skipped_tests_without_executing(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root)
            (path / "test_discovery_fixture.py").write_text('''import unittest
class Fixture(unittest.TestCase):
    def test_visible(self):
        raise AssertionError("discovery must not execute the test")
    @unittest.skip("explicit prerequisite")
    def test_ignored(self):
        raise AssertionError("skipped")
''')
            result = discovery.discover(path)
            self.assertEqual(result["tests"], [
                {"id": "test_discovery_fixture.Fixture.test_ignored", "ignored": True},
                {"id": "test_discovery_fixture.Fixture.test_visible", "ignored": False},
            ])

    def test_import_failure_is_not_a_test_identity(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root)
            (path / "test_import_failure_fixture.py").write_text('raise RuntimeError("broken")')
            with self.assertRaisesRegex(ValueError, "Python test discovery failed"):
                discovery.discover(path)

    def test_empty_suite_fails(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(ValueError, "nonempty"):
                discovery.discover(Path(root))
