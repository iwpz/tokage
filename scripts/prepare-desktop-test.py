#!/usr/bin/env python3
"""Prepare an external desktop test checkout; never launch the app or call a model.

Uses ordinary Tauri build configuration for storage isolation. Does not rewrite
source, stores, permissions, Git identity, CLI credentials or process environment.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import uuid

PROHIBITED_ENV = (
    'VITE_TOKAGE_LIVE_ACCEPTANCE', 'TOKAGE_LIVE_DESKTOP_CONFIG',
    'TOKAGE_DISABLE_SANDBOX', 'TOKAGE_BWRAP_PATH',
    'TOKAGE_IDLE_TIMEOUT_MS', 'TOKAGE_HARD_TIMEOUT_MS',
    'CLAUDE_CODE_DISABLE_AUTO_MEMORY', 'npm_config_cache',
)


def git(repo, *args):
    return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()


def prepare(source, requirement, parent=None, webview_storage='persistent'):
    if webview_storage not in ('persistent', 'ephemeral'):
        raise ValueError('Unknown WebView storage mode')
    source, requirement = source.resolve(), requirement.resolve()
    inherited = [name for name in PROHIBITED_ENV if name in os.environ]
    if inherited:
        raise ValueError('Remove undeclared test overrides before preparation: ' + ', '.join(inherited))
    if git(source, 'status', '--porcelain', '--untracked-files=no'):
        raise ValueError('Commit tracked changes before preparing the immutable test checkout')
    revision = git(source, 'rev-parse', 'HEAD')
    request_bytes = requirement.read_bytes()
    request = json.loads(request_bytes)
    if not isinstance(request, dict) or not request:
        raise ValueError('Requirement must be a nonempty preregistration JSON object')
    if request.get('protocol') != 'external-ui-v1':
        raise ValueError('Requirement must explicitly declare protocol: external-ui-v1')
    parent = (parent or Path(tempfile.gettempdir())).resolve()
    if parent == source or source in parent.parents:
        raise ValueError('Test output must be outside the source repository')
    root = Path(tempfile.mkdtemp(prefix='tokage-desktop-test-', dir=parent)).resolve()
    repo = root / 'repo'
    subprocess.run(['git', 'clone', '--no-hardlinks', '--no-checkout', str(source), str(repo)], check=True, capture_output=True)
    git(repo, 'checkout', '--detach', revision)
    config = json.loads((repo / 'src-tauri/tauri.conf.json').read_text())
    windows = config.get('app', {}).get('windows', [])
    # Preserve window behavior; only the WebView storage identity changes on macOS.
    isolated_windows = []
    for window in windows:
        isolated = dict(window)
        if webview_storage == 'ephemeral':
            isolated.pop('dataStoreIdentifier', None)
            isolated['incognito'] = True
        else:
            isolated['dataStoreIdentifier'] = list(uuid.uuid4().bytes)
        isolated_windows.append(isolated)
    overlay = {
        'identifier': 'dev.tokage.externaltest.' + uuid.uuid4().hex,
        'app': {'appDirectoriesOverride': str(root / 'app-data'), 'windows': isolated_windows},
    }
    (root / 'tauri-isolation.json').write_text(json.dumps(overlay, indent=2) + '\n')
    (root / 'request.json').write_bytes(request_bytes)
    record = {
        'sourceCommit': revision, 'protocol': 'external-ui-v1',
        'requirementSha256': hashlib.sha256(request_bytes).hexdigest(),
        'isolationOverlay': overlay, 'status': 'prepared-not-executed',
        'appLaunched': False, 'modelRequestSent': False,
        'webviewStorage': webview_storage,
        'limits': ['App data and WebView storage are isolated; CLI HOME, credentials and caches are not.',
                   'Use a dedicated OS test account if CLI writes must be isolated.',
                   'UI steps, permissions, model selection and delivery must use normal product controls.'],
    }
    if webview_storage == 'ephemeral':
        record['limits'].append('Incognito WebView settings do not persist across restarts; this cannot certify frontend settings persistence.')
    (root / 'preparation.json').write_text(json.dumps(record, indent=2) + '\n')
    commands = [
        'cd ' + shlex.quote(str(repo)),
        'npm ci --ignore-scripts --no-audit --no-fund',
        'python3 scripts/check-test-isolation.py',
        'npm run tauri -- build --no-bundle --config ' + shlex.quote(str(root / 'tauri-isolation.json')),
    ]
    (root / 'BUILD.txt').write_text('\n'.join(commands) + '\n\nBuild only. Start the resulting application manually, then follow docs/testing-isolation.md.\n')
    return root


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-repo', type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument('--requirement-file', type=Path, required=True)
    parser.add_argument('--output-parent', type=Path, help='Existing directory outside the source repository')
    parser.add_argument('--webview-storage', choices=('persistent', 'ephemeral'), default='persistent',
                        help='Standard Tauri storage mode; ephemeral cannot test frontend settings persistence')
    args = parser.parse_args()
    try:
        root = prepare(args.source_repo, args.requirement_file, args.output_parent, args.webview_storage)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.error(str(error))
    print(f'Prepared only: {root}\nRead BUILD.txt and docs/testing-isolation.md. No app or paid request was started.')


if __name__ == '__main__':
    main()
