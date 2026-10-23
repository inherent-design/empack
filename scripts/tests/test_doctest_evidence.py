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


def fixture(output, target="x86_64-unknown-linux-gnu", features="all"):
    """Invented observations for verifier tests; not an executed compiler run."""
    output.mkdir(parents=True, exist_ok=True)
    classification = json.loads((ROOT / BINDINGS).read_text())
    names = [row['id'].removeprefix('doctest::') for row in classification['tests']]
    listing = ''.join(name + ': test\n' for name in names) + f'{len(names)} tests, 0 benchmarks\n'
    run = f'running {len(names)} tests\n'
    for row, name in zip(classification['tests'], names):
        suffix = next(key for key, mode in MODES.items() if mode == row['mode'])
        run += f'test {name}{suffix} ... ok\n'
    run += f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n'
    for name, text in {'list.stdout': listing, 'ignored.stdout': '0 tests, 0 benchmarks\n',
                       'run.stdout': run, 'run.log': run, 'source-snapshot.json': '{}\n'}.items():
        (output / name).write_text(text, encoding='utf-8')
    expected = inventory(listing, '0 tests, 0 benchmarks\n', classification)
    tests, status, parse_error = evaluate(run, expected, 0, False)
    import tomllib
    pin = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']['channel']
    record = dict(schema=1, kind='doctest-suite-run', qualification=False, source_revision=REVISION,
                  worktree_dirty=False, target=target, platform=TARGETS[target], test_features=features,
                  created_at='2026-10-23T00:00:00+00:00', rustc=f'release: {pin}\nhost: {target}',
                  rustdoc=f'rustdoc {pin} (synthetic)', inputs=inputs(),
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


class RustdocAdmission(unittest.TestCase):
    def test_receipt_replays_exact_graph_modes_and_observations(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            record = fixture(path)
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
            self.assertIn('compile_fail', report.render_attempts(result))
