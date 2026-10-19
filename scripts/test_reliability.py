"""Regression tests for failure propagation, paid isolation and baseline drift."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch


def module(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parent / file)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


runner = module('reliability_runner', 'verify-reliability.py')
baselines = module('cli_baselines', 'check-cli-baselines.py')


class ReliabilityHarness(unittest.TestCase):
    def test_nonzero_missing_and_skipped_live_cannot_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            cases = [('failed', [sys.executable, '-c', 'raise SystemExit(7)'], True, 'failed'),
                     ('missing', ['/tokage/nonexistent/cli'], True, 'failed'),
                     ('skip', [sys.executable, '-c', 'print("skip target: missing")'], True, 'skipped'),
                     ('ok', [sys.executable, '-c', 'print("PASS fixture")'], False, 'passed')]
            for name, command, live, expected in cases:
                record = runner.run_check(name, command, output, live=live)
                self.assertEqual(record['status'], expected)
                self.assertTrue((output / record['log']).is_file())

    def test_failure_is_reported_and_later_checks_still_run(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'evidence'
            checks = [('bad', [sys.executable, '-c', 'raise SystemExit(9)']),
                      ('good', [sys.executable, '-c', 'print("PASS")'])]
            with patch.object(runner, 'local_checks', return_value=checks):
                self.assertEqual(runner.main(['--output', str(output)]), 1)
            report = json.loads((output / 'summary.json').read_text())
            self.assertEqual(report['counts'], {'passed': 1, 'failed': 1, 'skipped': 0})
            self.assertEqual(report['checks'][0]['exitCode'], 9)
            with self.assertRaises(FileExistsError):
                runner.main(['--output', str(output)])

    def test_paid_requires_opt_in_before_any_execution(self):
        with patch.object(runner, 'run_check') as run, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                runner.main(['--mode', 'live'])
            self.assertEqual(error.exception.code, 2)
            run.assert_not_called()

    def test_live_skipped_target_blocks_overall_acceptance(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'live'
            skipped = dict(name='CLAUDE_CODE', exitCode=0, status='skipped', skipLines=1)
            with patch.object(runner, 'run_check', return_value=skipped) as run:
                result = runner.main(['--mode', 'live', '--allow-paid', '--provider', 'CLAUDE_CODE', '--output', str(output)])
            self.assertEqual(result, 1)
            self.assertTrue(run.call_args.kwargs['live'])
            report = json.loads((output / 'summary.json').read_text())
            self.assertEqual(report['counts'], {'passed': 0, 'failed': 0, 'skipped': 1})

    def test_local_removes_inherited_paid_smoke_flag(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'TOKAGE_LIVE_SMOKE': '1'}):
            command = [sys.executable, '-c', 'import os; assert "TOKAGE_LIVE_SMOKE" not in os.environ']
            self.assertEqual(runner.run_check('env', command, Path(directory), live=False)['status'], 'passed')
        commands = runner.local_checks()
        self.assertFalse(any('--ignored' in command for _, command in commands))
        self.assertFalse(any('run-live' in part or 'run-m2b-desktop' in part for _, command in commands for part in command))

    def test_empty_rust_selection_is_not_success(self):
        with tempfile.TemporaryDirectory() as directory:
            result = runner.run_check('rust', [sys.executable, '-c', 'print("test result: ok. 0 passed; 0 failed; 0 ignored; 3 measured; 10 filtered out;")'], Path(directory), live=False)
            self.assertEqual(result['status'], 'failed')

    def test_failed_rust_suite_keeps_failed_test_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            output = 'test result: FAILED. 4 passed; 1 failed; 2 ignored; 0 measured; 3 filtered out;'
            record = runner.run_check('rust', [sys.executable, '-c', f'print({output!r})'], Path(directory), live=False)
            self.assertEqual(record['status'], 'failed')
            self.assertEqual(record['tests'], {'passed': 4, 'failed': 1, 'ignored': 2, 'filtered': 3})

    def test_selected_fixture_or_version_drift_requires_review(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = json.loads((runner.ROOT / 'scripts/cli-baselines.json').read_text())
            paths = ['scripts/cli-baselines.json', 'scripts/run-live-desktop.py', 'src/lib/reliabilitySupport.json']
            paths += [f['path'] for entry in manifest.values() for f in entry['fixtures']]
            for name in paths:
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(runner.ROOT / name, target)
            baselines.verify(root)
            fixture = root / next(iter(manifest.values()))['fixtures'][0]['path']
            saved = fixture.read_bytes()
            fixture.write_bytes(saved + b'\n')
            with self.assertRaisesRegex(AssertionError, 'fixture changed'):
                baselines.verify(root)
            fixture.write_bytes(saved)
            support = root / 'src/lib/reliabilitySupport.json'
            data = json.loads(support.read_text())
            data['targets'][0]['version'] = '999.0.0'
            support.write_text(json.dumps(data))
            with self.assertRaisesRegex(AssertionError, 'baseline changed'):
                baselines.verify(root)


if __name__ == '__main__':
    unittest.main()
