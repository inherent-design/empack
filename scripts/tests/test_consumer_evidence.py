"""Check consumer dependency admission and checkout-independent documentation validation."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def module(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / (name + '.py'))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


class ConsumerEvidence(unittest.TestCase):
    def test_only_supported_qt_versions_qualify(self):
        probe = module('check-prism-config')
        for version in ['6.4.0', '6.9.2', '6.12.0']:
            probe.require_supported_qt(version)
        for version in ['5.15.2', '6.3.9', '7.0.0', '6.4', '6.4.0-dev']:
            with self.assertRaises(ValueError):
                probe.require_supported_qt(version)

    def test_structural_docs_without_git_and_negative_cases(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'b.md').write_text('# Topic\n# Topic\n', encoding='utf-8')
            command = [sys.executable, str(ROOT / 'scripts/check-docs.py'), '--root', temp, 'a.md']
            cases = [('[link](b.md#topic-1)\n```md\n[ignored](missing)\n```\n', True),
                     ('[link](b.md#absent)\n', False), ('[link](absent.md)\n', False),
                     ('```rust\nunclosed\n', False)]
            for text, passes in cases:
                (root / 'a.md').write_text(text, encoding='utf-8')
                result = subprocess.run(command, capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, passes, result.stdout + result.stderr)

    def test_snapshot_scope_and_ancestry_are_explicit(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def git(*args):
                return subprocess.check_output(['git', '-C', temp, *args], text=True).strip()
            git('init', '-q')
            git('config', 'user.email', 'fixture@example.invalid')
            git('config', 'user.name', 'Fixture')
            (root / 'docs').mkdir()
            (root / 'docs/a.md').write_text('# Topic\n', encoding='utf-8')
            git('add', '.'); git('commit', '-qm', 'base')
            base = git('rev-parse', 'HEAD')
            (root / 'docs/a.md').write_text('# Topic\n[Self](#topic)\n', encoding='utf-8')
            git('add', '.'); git('commit', '-qm', 'docs')
            command = [sys.executable, str(ROOT / 'scripts/check-docs.py'), '--root', temp,
                       '--ref', 'HEAD', '--base', base, '--docs-only', 'docs']
            self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)
            (root / 'code.rs').write_text('// changed\n', encoding='utf-8')
            git('add', '.'); git('commit', '-qm', 'code')
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            command[command.index('--ref') + 1] = base
            command[command.index('--base') + 1] = 'HEAD'
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
