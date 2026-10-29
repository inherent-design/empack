"""Synthetic rustdoc records exercise admission, never supply execution evidence."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from verification.doctests import (BINDINGS, MODES, ROOT, TARGETS, command, evaluate, inputs,
                                  inspect_doctest_evidence, inventory, outcomes)
from verification.evidence import sha256, write_json

REVISION = "a" * 40


def fixture(output, target="x86_64-unknown-linux-gnu", features="all", source=ROOT):
    """Invented observations for verifier tests; not an executed compiler run."""
    output.mkdir(parents=True, exist_ok=True)
    classification = json.loads((source / BINDINGS).read_text())
    names = [row['id'].removeprefix('doctest::') for row in classification['tests']]
    listing = ''.join(name + ': test\n' for name in names) + f'{len(names)} tests, 0 benchmarks\n'
    run = f'running {len(names)} tests\n'
    for row, name in zip(classification['tests'], names):
        suffix = next(key for key, mode in MODES.items() if mode == row['mode'])
        run += f'test {name}{suffix} ... ok\n'
    run += f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n'
    if target == 'x86_64-pc-windows-msvc':
        # rustdoc emits native source separators in both discovery and execution.
        for name in names:
            path, separator, item = name.partition(' - ')
            native = path.replace('/', '\\') + separator + item
            listing = listing.replace(name, native)
            run = run.replace(name, native)
    for name, text in {'list.stdout': listing, 'ignored.stdout': '0 tests, 0 benchmarks\n',
                       'run.stdout': run, 'run.log': run, 'source-snapshot.json': '{}\n'}.items():
        (output / name).write_text(text, encoding='utf-8')
    expected = inventory(listing, '0 tests, 0 benchmarks\n', classification)
    tests, status, parse_error = evaluate(run, expected, 0, False)
    import tomllib
    pin = tomllib.loads((source / 'rust-toolchain.toml').read_text())['toolchain']['channel']
    record = dict(schema=1, kind='doctest-suite-run', qualification=False, source_revision=REVISION,
                  worktree_dirty=False, target=target, platform=TARGETS[target], test_features=features,
                  created_at='2026-10-23T00:00:00+00:00', rustc=f'release: {pin}\nhost: {target}',
                  rustdoc=f'rustdoc {pin} (synthetic)', inputs=inputs(source),
                  commands={action: command(features, action) for action in ('list', 'ignored', 'run')},
                  evidence={name: sha256(output / name) for name in ('list.stdout', 'ignored.stdout', 'run.stdout', 'run.log', 'source-snapshot.json')},
                  tests=tests, status=status, parse_error=parse_error, exit_code=0, timed_out=False)
    write_json(output / 'receipt.json', record)
    return record


class RustdocOutcomes(unittest.TestCase):
    def setUp(self):
        self.expected = {'crates/core.rs - Type (line 1)': {'mode': 'run', 'ignored': False}}
        self.output = ('running 1 test\ntest crates/core.rs - Type (line 1) ... ok\n'
                       'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n')

    def test_execution_compile_only_and_expected_failure_remain_distinct(self):
        for suffix, mode in MODES.items():
            expected = copy.deepcopy(self.expected)
            next(iter(expected.values()))['mode'] = mode
            rows, status = outcomes(self.output.replace(' ...', suffix + ' ...'), expected)
            self.assertEqual((rows[0]['mode'], status), (mode, 'PASS'))
        with self.assertRaisesRegex(ValueError, 'mode'):
            outcomes(self.output.replace(' ...', ' - compile ...'), self.expected)

    def test_missing_duplicate_unknown_filtered_and_inconsistent_output_fails(self):
        for text in ('', self.output.split('test result:')[0], self.output + self.output,
                     self.output.replace('Type', 'Unknown'), self.output.replace('0 filtered', '1 filtered'),
                     self.output.replace('1 passed', '2 passed'), self.output.replace('running 1', 'running 2'),
                     self.output + '[output truncated at admitted log limit]\n'):
            with self.subTest(text=text):
                rows, status, error = evaluate(text, self.expected, 0, False)
                self.assertEqual((rows, status), ([], 'FAIL'))
                self.assertTrue(error)

    def test_skip_failed_assertion_timeout_and_exit_failure_cannot_pass(self):
        expected = copy.deepcopy(self.expected)
        next(iter(expected.values()))['ignored'] = True
        skip = self.output.replace('... ok', '... ignored').replace('1 passed; 0 failed; 0 ignored', '0 passed; 0 failed; 1 ignored')
        self.assertEqual(outcomes(skip, expected)[1], 'NOT_RUN')
        fail = self.output.replace('... ok', '... FAILED').replace('result: ok', 'result: FAILED').replace('1 passed; 0 failed', '0 passed; 1 failed')
        self.assertEqual(outcomes(fail, self.expected)[1], 'FAIL')
        for code, timed_out in ((1, False), (0, True)):
            self.assertEqual(evaluate(self.output, self.expected, code, timed_out)[1], 'FAIL')
        # Unparsed compiler/test failure diagnostics are retained, never ignored.
        self.assertEqual(evaluate(fail + 'failures:\ncompiler diagnostic\n', self.expected, 101, False)[1], 'FAIL')

    def test_listing_is_not_execution_and_ignore_policy_is_explicit(self):
        listing = 'crates/core.rs - Type (line 1): test\n1 test, 0 benchmarks\n'
        bindings = {'tests': [{'id': 'doctest::crates/core.rs - Type (line 1)', 'mode': 'run', 'ignored': False}]}
        self.assertEqual(inventory(listing, '0 tests, 0 benchmarks\n', bindings), {
            'crates/core.rs - Type (line 1)': bindings['tests'][0]})
        with self.assertRaises(ValueError):
            inventory(listing, listing, bindings)
        with self.assertRaises(ValueError):
            outcomes(listing, self.expected)
        windows = listing.replace('crates/core.rs', 'crates\\core.rs')
        self.assertEqual(inventory(windows, '0 tests, 0 benchmarks\n', bindings),
                         inventory(listing, '0 tests, 0 benchmarks\n', bindings))
        self.assertEqual(outcomes(self.output.replace('crates/core.rs', 'crates\\core.rs'), self.expected),
                         outcomes(self.output, self.expected))
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            inventory(listing + windows, '0 tests, 0 benchmarks\n', bindings)
        duplicate = self.output + self.output.replace('crates/core.rs', 'crates\\core.rs')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            outcomes(duplicate, self.expected)
        for source in ('other/core.rs', 'crates//core.rs', 'C:\\crates\\core.rs', 'crates/../core.rs'):
            with self.subTest(source=source), self.assertRaises(ValueError):
                inventory(listing.replace('crates/core.rs', source), '0 tests, 0 benchmarks\n', bindings)
        with self.assertRaises(ValueError):
            outcomes(self.output.replace('Type', 'Type\\Nested'), self.expected)


class RustdocAdmission(unittest.TestCase):
    def test_receipt_replays_exact_graph_modes_and_observations(self):
        with tempfile.TemporaryDirectory() as directory:
            for target in ('x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc'):
                with self.subTest(target=target):
                    path = Path(directory) / target
                    record = fixture(path, target=target)
                    admitted = inspect_doctest_evidence(path / 'receipt.json', REVISION)
                    self.assertEqual(admitted['tests'], record['tests'])
                    self.assertEqual(admitted['status'], 'PASS')
                    self.assertEqual({row['mode'] for row in admitted['tests']}, {'run', 'compile', 'compile_fail'})

    def test_changed_source_toolchain_command_and_outcomes_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            original = fixture(path)
            for key, value in [('source_revision', 'b' * 40), ('worktree_dirty', True), ('test_features', 'minimal'),
                               ('rustdoc', 'rustdoc 1.80.0 (other)'), ('inputs', {}), ('tests', []),
                               ('timed_out', 0), ('status', 'FAIL'), ('reason', 'source changed')]:
                with self.subTest(key=key):
                    write_json(path / 'receipt.json', {**original, key: value})
                    with self.assertRaises(ValueError):
                        inspect_doctest_evidence(path / 'receipt.json', REVISION)
            write_json(path / 'receipt.json', original)
            (path / 'run.stdout').write_text('changed output')
            with self.assertRaisesRegex(ValueError, 'changed'):
                inspect_doctest_evidence(path / 'receipt.json', REVISION)

    def test_report_retains_failed_attempt_alongside_later_pass(self):
        spec = importlib.util.spec_from_file_location('report_doctest_test', ROOT / 'scripts/report-verification.py')
        report = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(report)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            failed = fixture(root / 'failed')
            failed.update(exit_code=101, status='FAIL')
            write_json(root / 'failed/receipt.json', failed)
            fixture(root / 'pass')
            result = report.report([root / 'failed/receipt.json', root / 'pass/receipt.json'], REVISION, '0.8.4-beta')
            self.assertEqual(result['status'], 'FAIL')
            self.assertEqual([row['status'] for row in result['doctest_evidence']], ['FAIL', 'PASS'])
            rendered = report.render_attempts(result)
            self.assertIn('| Test graph | Rustdoc mode |', rendered)
            self.assertIn('<code>all</code> | <code>compile_fail</code>', rendered)
            self.assertIn('Rustdoc run:', rendered)


class RustdocAcceptance(unittest.TestCase):
    """Already-admitted synthetic observations test the requirement join only."""
    def setUp(self):
        from verification.requirements import case_results
        self.join = case_results
        self.identity = 'doctest::crates/empack-core/src/path.rs - path::PortableRelPath (line 20)'
        self.case = dict(id='UX-086.constructor', boundary='U', test_ids=[self.identity],
                         required_platforms=['linux', 'macos', 'windows'], required_product_features=['independent'],
                         required_test_features=['minimal', 'all'], doctest_mode='compile_fail')
        self.document = dict(schema=1, requirements=[dict(id='UX-086', scope='stable', risk='critical',
                             evidence_tiers=['U'], case_bindings_status='complete', cases=[self.case])])
        self.classification = dict(targets=TARGETS, tests=[])
        self.records = []
        for index, target in enumerate(('x86_64-unknown-linux-gnu', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc')):
            for graph_index, graph in enumerate(('minimal', 'all')):
                self.records.append(dict(kind='doctest', source_revision=REVISION, status='PASS',
                    target=target, platform=TARGETS[target], test_features=graph,
                    receipt_sha256=str(index * 2 + graph_index) * 64,
                    created_at='2026-10-23T00:00:00+00:00', rustdoc='synthetic tool identity',
                    source_snapshot_sha256='c' * 64,
                    tests=[dict(id=self.identity, mode='compile_fail', status='PASS')]))

    def result(self):
        return self.join(self.document, self.classification, [], REVISION, doctest_evidence=self.records)['requirements'][0]

    def test_each_host_and_test_graph_is_required_without_claiming_a_product(self):
        result = self.result()
        self.assertEqual(result['status'], 'PASS')
        variants = result['cases'][0]['variants']
        self.assertEqual(len(variants), 6)
        for variant in variants:
            self.assertEqual(variant['features'], 'independent')
            self.assertEqual(variant['doctest_mode'], 'compile_fail')
            test = variant['tests'][0]
            self.assertEqual(test['test_features'], variant['test_features'])
            self.assertNotIn('artifact_sha256', test)
        for index in range(6):
            saved = self.records.pop(index)
            self.assertEqual(self.result()['status'], 'NOT_RUN')
            self.records.insert(index, saved)

    def test_failed_attempt_with_no_parseable_rows_survives_a_later_pass(self):
        failed = dict(self.records[0], receipt_sha256='f' * 64, status='FAIL', tests=[])
        self.records.append(failed)
        result = self.result()
        self.assertEqual(result['status'], 'FAIL')
        attempts = result['cases'][0]['variants'][0]['tests'][0]['attempts']
        self.assertEqual({row['status'] for row in attempts}, {'PASS', 'FAIL'})
        self.records.reverse()
        self.assertEqual(self.result(), result)
        self.records.remove(failed)
        self.document['requirements'][0]['case_bindings_status'] = 'partial'
        self.assertEqual(self.result()['status'], 'NOT_RUN', 'passing examples do not complete the broader requirement')

    def test_mode_source_graph_identity_and_receipt_conflicts_are_rejected(self):
        original = copy.deepcopy(self.records)
        for field, value in [('source_revision', 'b' * 40), ('platform', 'windows'),
                             ('test_features', 'unknown'), ('kind', 'python'), ('status', 'WAIVED')]:
            self.records = copy.deepcopy(original)
            self.records[0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.result()
        self.records = copy.deepcopy(original)
        self.records[0]['tests'][0]['mode'] = 'run'
        with self.assertRaisesRegex(ValueError, 'wrong-mode'):
            self.result()
        self.records = copy.deepcopy(original)
        self.records[0]['tests'].append(dict(self.records[0]['tests'][0]))
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            self.result()
        self.records = copy.deepcopy(original)
        self.records.append(dict(self.records[0], tests=[]))
        with self.assertRaisesRegex(ValueError, 'Conflicting'):
            self.result()

    def test_case_cannot_omit_graph_or_substitute_mode_product_or_boundary(self):
        original = copy.deepcopy(self.case)
        for field, value in [('required_test_features', []), ('required_test_features', ['all', 'all']),
                             ('required_product_features', ['default']), ('doctest_mode', 'run'),
                             ('boundary', 'X')]:
            self.case.update(original)
            self.case[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.result()
        self.case.update(original)
        del self.case['doctest_mode']
        with self.assertRaises(ValueError):
            self.result()
