"""Isolation boundary regressions: refuse hooks, preserve source and fail closed."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


fence = module('isolation_fence', 'check-test-isolation.py')
prep = module('desktop_preparation', 'prepare-desktop-test.py')


class Isolation(unittest.TestCase):
    def test_regression_hooks_and_external_test_imports_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'src').mkdir()
            (root / 'src-tauri').mkdir()
            (root / 'src-tauri/Cargo.toml').write_text('[features]\n')
            path = root / 'src/entry.ts'
            for code in [
                'import "../tests/driver";',
                'import {run} from "../tests/driver";',
                'import {run} from "../scripts/driver";',
                'if (config.scenario === "m3a") approve();',
                'invoke("get_live_acceptance_config");',
                'const env = "TOKAGE_DISABLE_SANDBOX";',
            ]:
                path.write_text(code)
                self.assertTrue(fence.violations(root), code)
            path.write_text('export const ordinary = true;')
            self.assertEqual(fence.violations(root), [])
            (root / 'cli/src').mkdir(parents=True)
            (root / 'cli/src/main.rs').write_text('const BAD: &str = \"TOKAGE_DISABLE_SANDBOX\";')
            self.assertTrue(fence.violations(root))
            (root / 'cli/src/main.rs').write_text('fn main() {}')
            self.assertEqual(fence.violations(root), [])

    def fixture(self, root):
        source = root / 'source'
        source.mkdir()
        (source / 'src-tauri').mkdir()
        (source / 'src-tauri/tauri.conf.json').write_text(json.dumps({'app': {'windows': [{'title': 'Tokage', 'width': 1280}]}}))
        def git(*args):
            return subprocess.run(['git', '-C', str(source), *args], check=True, capture_output=True)
        git('init', '-b', 'main')
        git('add', '.')
        # Fixture identity is command-scoped; never writes user/local Git config.
        git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgsign=false', 'commit', '-m', 'fixture')
        request = root / 'request.json'
        request.write_text('{"id":"example","goal":"normal UI","protocol":"external-ui-v1"}')
        return source, request

    def test_preparation_is_external_immutable_and_not_a_live_success(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {}, clear=True):
            root = Path(directory)
            source, request = self.fixture(root)
            before_config = (source / '.git/config').read_bytes()
            before_head = prep.git(source, 'rev-parse', 'HEAD')
            evidence = prep.prepare(source, request, root)
            record = json.loads((evidence / 'preparation.json').read_text())
            self.assertEqual(record['status'], 'prepared-not-executed')
            self.assertFalse(record['appLaunched'])
            self.assertFalse(record['modelRequestSent'])
            self.assertEqual(prep.git(evidence / 'repo', 'rev-parse', 'HEAD'), before_head)
            self.assertEqual(prep.git(evidence / 'repo', 'status', '--porcelain'), '')
            self.assertEqual((source / '.git/config').read_bytes(), before_config)
            self.assertEqual((evidence / 'request.json').read_bytes(), request.read_bytes())
            overlay = json.loads((evidence / 'tauri-isolation.json').read_text())
            self.assertEqual(overlay['app']['appDirectoriesOverride'], str(evidence / 'app-data'))
            self.assertEqual(overlay['app']['windows'][0]['width'], 1280)
            self.assertEqual(len(overlay['app']['windows'][0]['dataStoreIdentifier']), 16)

    def test_ephemeral_storage_is_explicit_and_cannot_claim_settings_persistence(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {}, clear=True):
            root = Path(directory)
            source, request = self.fixture(root)
            before = (source / 'src-tauri/tauri.conf.json').read_bytes()
            evidence = prep.prepare(source, request, root, 'ephemeral')
            record = json.loads((evidence / 'preparation.json').read_text())
            self.assertEqual(record['webviewStorage'], 'ephemeral')
            self.assertFalse(record['appLaunched'])
            self.assertFalse(record['modelRequestSent'])
            self.assertTrue(any('do not persist' in limit for limit in record['limits']))
            window = record['isolationOverlay']['app']['windows'][0]
            self.assertTrue(window['incognito'])
            self.assertNotIn('dataStoreIdentifier', window)
            self.assertEqual(window['width'], 1280)
            self.assertEqual((source / 'src-tauri/tauri.conf.json').read_bytes(), before)
            self.assertEqual(prep.git(evidence / 'repo', 'status', '--porcelain'), '')
            with self.assertRaisesRegex(ValueError, 'storage mode'):
                prep.prepare(source, request, root, 'unknown')

    def test_dirty_source_nested_output_and_inherited_override_are_refused(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {}, clear=True):
            root = Path(directory)
            source, request = self.fixture(root)
            with self.assertRaisesRegex(ValueError, 'outside'):
                prep.prepare(source, request, source)
            saved_request = request.read_text()
            request.write_text('{"id":"legacy"}')
            with self.assertRaisesRegex(ValueError, 'protocol'):
                prep.prepare(source, request, root)
            request.write_text(saved_request)
            with patch.dict(os.environ, {'TOKAGE_DISABLE_SANDBOX': '0'}):
                with self.assertRaisesRegex(ValueError, 'overrides'):
                    prep.prepare(source, request, root)
            (source / 'src-tauri/tauri.conf.json').write_text('{}')
            with self.assertRaisesRegex(ValueError, 'Commit tracked'):
                prep.prepare(source, request, root)

    def test_retired_launchers_cannot_build_or_send_paid_requests(self):
        root = Path(__file__).resolve().parent.parent
        for filename in ['run-m3a-desktop.py', 'run-live-desktop.py', 'run-m2g-crash.py',
                         'run-live-desktop.sh', 'run-m2b-desktop.sh', 'run-m2g-desktop.sh']:
            executable = 'bash' if filename.endswith('.sh') else os.sys.executable
            result = subprocess.run([executable, str(root / 'scripts' / filename)], cwd='/tmp', capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Retired', result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
