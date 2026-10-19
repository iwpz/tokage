#!/usr/bin/env python3
"""Paid selected-model desktop SIGKILL / cold recovery, against isolated fixtures.
Build the explicit acceptance bundle first. Never alters account configuration.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import tempfile
import time
import runpy

ROOT = Path(__file__).resolve().parent.parent
BASE = runpy.run_path(str(ROOT / 'scripts/run-live-desktop.py'))


def receipts(root):
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in root.glob('app-data/**/turn-receipts/*.json')}


def wait_event(root, process, step, timeout):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        path = root / 'progress.jsonl'
        for line in path.read_text().splitlines() if path.exists() else []:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get('step') == step:
                return event
            if event.get('status') == 'failed' or (step == 'crash-ready' and event.get('step') == 'native-error'):
                raise RuntimeError(f'Real CLI or desktop failure before {step}; evidence: {root}')
        if process.poll() is not None:
            raise RuntimeError(f'Desktop exited {process.returncode} before {step}; evidence: {root}')
        time.sleep(.1)
    raise TimeoutError(f'Timed out waiting for {step}; evidence: {root}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('provider', choices=['CLAUDE_CODE', 'OPENCODE'])
    parser.add_argument('--allow-paid', action='store_true')
    parser.add_argument('--cli-path', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=ROOT / 'src-tauri/target/debug/tokage')
    args = parser.parse_args()
    if not args.allow_paid:
        parser.error('--allow-paid is required: this test sends a real model request')
    baseline = json.loads((ROOT / 'scripts/cli-baselines.json').read_text())[args.provider]
    version = subprocess.run([str(args.cli_path), '--version'], capture_output=True, text=True, check=True).stdout.strip()
    expected = baseline['version']
    if version not in [f'{expected} (Claude Code)', f'opencode v{expected}', expected]:
        parser.error(f'Wrong exact CLI version: {version}')
    root = Path(tempfile.mkdtemp(prefix='tokage-live-desktop-', dir='/tmp')).resolve()
    (root / '.tokage-live-acceptance').write_text('Tokage live CLI acceptance\n')
    repo, before, files = BASE['fixture'](root)
    config = dict(fault='crash', scenario='fixed', cliPath=str(args.cli_path.resolve()),
                  root=str(root), cwd=str(repo), provider=args.provider, model=baseline['model'], tier=None,
                  versionPrefix=expected)
    path = root / 'config.json'
    path.write_text(json.dumps(config))
    env = dict(os.environ, TOKAGE_LIVE_DESKTOP_CONFIG=str(path))
    print(f'START crash {args.provider} {baseline["model"]} fixture={root}', flush=True)
    process = None
    try:
        with (root / 'desktop-before.log').open('w') as log:
            process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=subprocess.STDOUT)
            event = wait_event(root, process, 'crash-ready', 300)
            claims = receipts(root)
            if not claims:
                raise RuntimeError('No durable launch receipt before SIGKILL')
            registries = list(root.glob('app-data/**/run/children.json'))
            records = [record for p in registries for record in json.loads(p.read_text())]
            if not records:
                raise RuntimeError('No registered native process before SIGKILL')
            (root / 'crash-before.json').write_text(json.dumps(dict(event=event, receipts=claims, children=records), indent=2))
            process.kill()
            process.wait(timeout=15)
            if process.returncode != -signal.SIGKILL:
                raise RuntimeError('Host was not killed by SIGKILL')
        config['recovery'] = dict(sessionId=event['sessionId'], cwd=event['deliveryCheckout'])
        path.write_text(json.dumps(config))
        with (root / 'desktop-after.log').open('w') as log:
            process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=subprocess.STDOUT)
            result = wait_event(root, process, 'crash-recovered-blocked', 180)
            process.wait(timeout=15)
            if process.returncode != 0:
                raise RuntimeError(f'Recovery exited {process.returncode}')
        if receipts(root) != claims:
            raise RuntimeError('Recovery added or changed model dispatch receipts')
        for record in records:
            for pid in [-record['pgid'], record['pid']]:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    continue
                raise RuntimeError(f'Process survived startup cleanup: {pid}')
        if BASE['git'](repo, 'rev-parse', 'HEAD') != before:
            raise RuntimeError('Recovery changed delivery checkout HEAD')
        if any((repo / name).read_text() != text for name, text in files.items()):
            raise RuntimeError('Recovery changed original fixture files')
        with sqlite3.connect(root / 'app-data/tokage.db') as db:
            db.row_factory = sqlite3.Row
            attribution = {name: [dict(row) for row in db.execute(query)] for name, query in {
                'attempts': 'SELECT id,provider_name,model_id,status,workdir,task_id FROM attempts',
                'usage': 'SELECT attempt_id,provider_name,model_id,input_tokens,output_tokens,cost_usd FROM usage_events',
                'unknownCost': 'SELECT turn_id,plan_id FROM recovery_usage_gaps',
            }.items()}
        recovered = result['before']['recoveryTasks']
        expected_turns = {event['snapshot']['taskTurnIds'][tid] for tid in recovered}
        task_attempts = [row for row in attribution['attempts'] if row['task_id'] is not None]
        if {row['id'] for row in task_attempts} != expected_turns or any(row['status'] == 'DONE' for row in task_attempts):
            raise RuntimeError('Database attempt identity mismatch or false DONE')
        if not expected_turns.issubset({row['turn_id'] for row in attribution['unknownCost']}):
            raise RuntimeError('Interrupted real requests lost unknown-cost attribution')
        (root / 'database-attribution.json').write_text(json.dumps(attribution, indent=2))
        proof = dict(status='passed', scenario='desktop-sigkill-cold-recovery', provider=args.provider,
                     version=version, model=baseline['model'], sourceCommit=BASE['git'](ROOT, 'rev-parse', 'HEAD'),
                     sourceHashes={str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in [ROOT / 'src/lib/liveDesktopAcceptance.ts', ROOT / 'src-tauri/src/live_acceptance.rs', Path(__file__)]},
                     hostExit=-signal.SIGKILL, noNewDispatchReceipts=True, processesReaped=True,
                     originalCheckoutPreserved=True, before=event, recovery=result)
        (root / 'verified-crash.json').write_text(json.dumps(proof, indent=2))
        print(f'PASS crash {args.provider}; evidence: {root}', flush=True)
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)


if __name__ == '__main__':
    main()
