"""The crash observer must reject real errors, early exit and unrelated progress."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location('m2g_crash', Path(__file__).with_name('run-m2g-crash.py'))
crash = importlib.util.module_from_spec(spec)
spec.loader.exec_module(crash)


class CrashObserver(unittest.TestCase):
    def test_provider_error_stops_before_injection(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'progress.jsonl').write_text(json.dumps({'step': 'native-error', 'status': 'running'}) + '\n')
            with self.assertRaisesRegex(RuntimeError, 'Real CLI'):
                crash.wait_event(root, Mock(), 'crash-ready', 1)

    def test_early_exit_is_failure_not_skip(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, 'exited 0'):
                crash.wait_event(Path(directory), Mock(returncode=0, poll=lambda: 0), 'crash-ready', 1)

    def test_partial_report_does_not_count_as_success(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'progress.jsonl').write_text('{"step": "crash-ready"')
            with self.assertRaises(TimeoutError), patch.object(crash.time, 'monotonic', side_effect=[0, 0, 2]):
                crash.wait_event(root, Mock(poll=lambda: None), 'crash-ready', 1)

    def test_recovery_observer_ignores_previous_stage(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            passed = dict(step='crash-recovered-blocked', status='passed')
            (root / 'progress.jsonl').write_text('\n'.join(map(json.dumps, [dict(step='crash-ready'), passed])))
            self.assertEqual(crash.wait_event(root, Mock(), 'crash-recovered-blocked', 1), passed)

    def test_dispatch_receipt_change_is_detectable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'app-data/turn-receipts/first.json'
            path.parent.mkdir(parents=True)
            path.write_text('{"turnId":"first"}')
            before = crash.receipts(root)
            path.write_text('{"turnId":"changed"}')
            self.assertNotEqual(before, crash.receipts(root))
            path.with_name('second.json').write_text('{}')
            self.assertEqual(len(crash.receipts(root)), 2)


class EvidenceGate(unittest.TestCase):
    def fixture(self, root):
        spec = importlib.util.spec_from_file_location('m2g_evidence', Path(__file__).with_name('verify-m2g-evidence.py'))
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        scenarios = [dict(id=f'{milestone}.{i}', status='passed', reason='local fixture', evidence=['proof.txt'])
                     for milestone, count in [('m2b', 5), ('m2c', 5), ('m2d', 6), ('m2e', 4)] for i in range(1, count + 1)]
        for provider in ['CLAUDE_CODE', 'OPENCODE']:
            for kind in ['natural-desktop', 'desktop-crash']:
                scenarios.append(dict(id=f'{provider}.{kind}', status='untested', reason='no live evidence', evidence=[]))
        path = root / gate.MANIFEST
        path.parent.mkdir(parents=True)
        (root / 'proof.txt').write_text('local fixture only')
        import hashlib
        manifest = dict(targets={'CLAUDE_CODE': {}, 'OPENCODE': {}}, scenarios=scenarios, complete=False,
                        liveProofs={}, sha256={'proof.txt': hashlib.sha256((root / 'proof.txt').read_bytes()).hexdigest()})
        path.write_text(json.dumps(manifest))
        return gate, path, manifest

    def test_missing_live_blocks_certification_even_if_integrity_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gate, _, _ = self.fixture(root)
            gate.verify(root)
            with self.assertRaisesRegex(ValueError, 'incomplete'):
                gate.verify(root, require_complete=True)

    def test_invented_completion_or_pass_cannot_bypass_missing_proof(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gate, path, manifest = self.fixture(root)
            manifest['complete'] = True
            path.write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):
                gate.verify(root)
            manifest['scenarios'][-1]['status'] = 'passed'
            manifest['scenarios'][-1]['evidence'] = ['proof.txt']
            manifest['complete'] = False
            path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, 'no machine-readable'):
                gate.verify(root)

    def test_forged_success_cannot_hide_downstream_execution(self):
        import hashlib
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gate, path, manifest = self.fixture(root)
            for provider in manifest['targets']:
                paths = {}
                for kind in ['natural-desktop', 'desktop-crash']:
                    name = f'{provider}-{kind}.json'
                    if kind == 'natural-desktop':
                        proof = dict(provider=provider, proof=dict(status='passed', gateCount=1))
                    else:
                        proof = dict(provider=provider, status='passed', hostExit=-9, noNewDispatchReceipts=True,
                                     processesReaped=True, originalCheckoutPreserved=True,
                                     before=dict(snapshot=dict(taskStates={'T1': 'RUNNING', 'T2': 'PENDING'}, completionReceipts={}, taskTurnIds={'T1': 'turn'})),
                                     recovery=dict(status='passed', step='crash-recovered-blocked',
                                                   snapshot=dict(taskStates={'T1': 'BLOCKED', 'T2': 'PENDING'}, taskTurnIds={'T1': 'turn'}),
                                                   evidence={'T1': dict(runtimeIdle=True, launchRecorded=True)}))
                    (root / name).write_text(json.dumps(proof))
                    paths[kind] = name
                    manifest['sha256'][name] = hashlib.sha256((root / name).read_bytes()).hexdigest()
                    entry = next(e for e in manifest['scenarios'] if e['id'] == f'{provider}.{kind}')
                    entry.update(status='passed', evidence=[name])
                manifest['liveProofs'][provider] = paths
            manifest['complete'] = True
            path.write_text(json.dumps(manifest))
            gate.verify(root, require_complete=True)
            name = manifest['liveProofs']['OPENCODE']['desktop-crash']
            proof = json.loads((root / name).read_text())
            proof['recovery']['snapshot']['taskStates']['T2'] = 'DONE'
            (root / name).write_text(json.dumps(proof))
            manifest['sha256'][name] = hashlib.sha256((root / name).read_bytes()).hexdigest()
            path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, 'downstream'):
                gate.verify(root, require_complete=True)
            proof['recovery']['snapshot']['taskStates']['T2'] = 'PENDING'
            proof['recovery']['evidence'] = {}
            (root / name).write_text(json.dumps(proof))
            manifest['sha256'][name] = hashlib.sha256((root / name).read_bytes()).hexdigest()
            path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, 'native idle/launch'):
                gate.verify(root, require_complete=True)

    def test_modified_or_missing_archive_blocks_integrity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gate, _, _ = self.fixture(root)
            (root / 'proof.txt').write_text('changed evidence')
            with self.assertRaisesRegex(ValueError, 'drift'):
                gate.verify(root)
            (root / 'proof.txt').unlink()
            with self.assertRaises(FileNotFoundError):
                gate.verify(root)


if __name__ == '__main__':
    unittest.main()
