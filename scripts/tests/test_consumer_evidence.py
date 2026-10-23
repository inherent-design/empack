"""Check consumer dependency admission and checkout-independent documentation validation."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))


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


class ParserAdmission(unittest.TestCase):
    def record(self):
        from consumer_fixture import prism_record
        return prism_record('a' * 40)

    def test_observed_settings_are_checked_instead_of_the_claimed_pass(self):
        import copy
        from verification.consumer import inspect_prism_record
        original = self.record()
        for key, value in [('override', False), ('loaded', False), ('prepare', 'wrong command'), ('wrapper', 'wrong wrapper')]:
            record = copy.deepcopy(original)
            record['fixtures'][0]['consumer'][key] = value
            with self.subTest(key=key):
                with self.assertRaisesRegex(ValueError, 'status disagrees'):
                    inspect_prism_record(record, 'a' * 40)
                record['status'] = 'FAIL'
                result = inspect_prism_record(record, 'a' * 40)
                self.assertEqual(result['status'], 'FAIL')
                self.assertEqual(result['failed_fixtures'], ['supported-1.2'])

    def test_probe_inputs_environment_and_complete_observations_are_required(self):
        import copy
        from verification.consumer import inspect_prism_record
        original = self.record()
        for mutate in [
            lambda r: r['inputs'].pop('scripts/check-prism-config.py'),
            lambda r: r['inputs'].update({'scripts/consumer/prism-config-probe.cpp': '0' * 64}),
            lambda r: r.update(probe_sha256='not-a-digest'),
            lambda r: r.update(created_at='2026-10-23T00:00:00'),
            lambda r: r['environment'].update(cplusplus=201703),
            lambda r: r['build_configuration'].update(mode='debug'),
            lambda r: r['build_configuration'].update(CXX=''),
            lambda r: r.update(gameplay_tested=True),
            lambda r: r.update(host_system='unobserved'),
            lambda r: r['fixtures'].append(r['fixtures'][0]),
            lambda r: r['fixtures'][0]['consumer'].update(override='true'),
            lambda r: r['fixtures'][0].pop('consumer'),
            lambda r: r.update(environment=None),
        ]:
            record = copy.deepcopy(original)
            mutate(record)
            with self.subTest(record=record.get('probe_sha256')), self.assertRaises(ValueError):
                inspect_prism_record(record, 'a' * 40)

    def test_declared_negative_cases_are_rechecked(self):
        from verification.consumer import inspect_prism_record, PREPARE, WRAPPER
        record = self.record()
        shadow = next(x for x in record['fixtures'] if x['fixture'] == 'legacy-shadow')
        shadow['consumer'].update(loaded=True, override=True, prepare=PREPARE, wrapper=WRAPPER)
        record['status'] = 'FAIL'
        self.assertIn('legacy-shadow', inspect_prism_record(record, 'a' * 40)['failed_fixtures'])
        record = self.record()
        negative = next(x for x in record['fixtures'] if not x['supported'] and 'hash' in x['fixture'])
        negative['consumer']['override'] = True
        with self.assertRaises(ValueError):
            inspect_prism_record(record, 'a' * 40)

    def test_file_receipt_is_bounded_and_binds_exact_bytes(self):
        import json
        from verification.consumer import inspect_prism_evidence
        from verification.evidence import sha256
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'prism.json'
            path.write_text(json.dumps(self.record()))
            result = inspect_prism_evidence(path, 'a' * 40)
            self.assertEqual(result['receipt_sha256'], sha256(path))
            self.assertEqual(result['features'], 'independent')
            self.assertNotIn('artifact_sha256', result)
            for payload in ('', ' ' * (1024 * 1024 + 1)):
                path.write_text(payload)
                with self.assertRaisesRegex(ValueError, 'nonempty and at most'):
                    inspect_prism_evidence(path, 'a' * 40)

    def test_parser_case_has_its_own_host_and_cannot_satisfy_product_execution(self):
        import copy
        from verification.consumer import inspect_prism_record, PRISM_TEST
        from verification.requirements import case_results
        record = inspect_prism_record(self.record(), 'a' * 40)
        record['receipt_sha256'] = 'b' * 64
        case = dict(id='UX-053.parser', boundary='C', test_ids=[PRISM_TEST],
                    required_platforms=['linux'], required_product_features=['independent'])
        document = dict(schema=1, requirements=[dict(id='UX-053', scope='stable', risk='critical',
                        evidence_tiers=['C'], case_bindings_status='partial', cases=[case])])
        classification = dict(targets={'linux-native': 'linux'}, tests=[])
        def result(records):
            return case_results(document, classification, [], 'a' * 40, consumer_evidence=records)['requirements'][0]
        self.assertEqual(result([])['cases'][0]['status'], 'NOT_RUN')
        joined = result([record])
        self.assertEqual(joined['cases'][0]['status'], 'PASS')
        self.assertEqual(joined['status'], 'NOT_RUN')
        attempt = joined['cases'][0]['variants'][0]['tests'][0]['attempts'][0]
        self.assertEqual(attempt['probe_sha256'], record['probe_sha256'])
        self.assertNotIn('artifact_sha256', attempt)
        other_host = dict(record, platform='macos')
        self.assertEqual(result([other_host])['cases'][0]['status'], 'NOT_RUN')
        failed = dict(record, receipt_sha256='c' * 64, status='FAIL')
        self.assertEqual(result([failed, record])['status'], 'FAIL')
        for bad in [dict(record, source_revision='d' * 40), dict(record, features='default')]:
            with self.assertRaises(ValueError):
                result([bad])
        for field, value in [('boundary', 'X'), ('required_product_features', ['default']),
                             ('test_ids', ['consumer::unreviewed::claim'])]:
            changed = copy.deepcopy(case)
            case[field] = value
            with self.assertRaises(ValueError):
                result([record])
            case.clear(); case.update(changed)
