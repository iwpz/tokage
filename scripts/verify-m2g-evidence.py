#!/usr/bin/env python3
"""Offline integrity and scope validation; --require-complete gates M2 acceptance.
Hashes detect archive drift; they do not independently certify real model behavior.
"""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = 'docs/reliability-evidence/m2g/manifest.json'


def verify(root=ROOT, require_complete=False):
    manifest = json.loads((root / MANIFEST).read_text())
    targets = {'CLAUDE_CODE', 'OPENCODE'}
    if set(manifest['targets']) != targets:
        raise ValueError('Selected provider scope changed')
    entries = manifest['scenarios']
    ids = [entry['id'] for entry in entries]
    if len(set(ids)) != len(ids):
        raise ValueError('Duplicate scenario identity')
    for entry in entries:
        if entry['status'] not in {'passed', 'failed', 'untested'} or not entry.get('reason'):
            raise ValueError('Every scenario needs a result and reason')
        if entry['status'] != 'untested' and not entry['evidence']:
            raise ValueError('Executed scenario has no evidence')
        for evidence in entry['evidence']:
            path = (root / evidence).resolve()
            if not path.is_relative_to(root.resolve()) or evidence not in manifest['sha256']:
                raise ValueError(f'Unpinned or escaping evidence: {evidence}')
    for name, expected in manifest['sha256'].items():
        path = (root / name).resolve()
        if not path.is_relative_to(root.resolve()) or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError(f'Evidence drift: {name}')
    for milestone, count in {'m2b': 5, 'm2c': 5, 'm2d': 6, 'm2e': 4}.items():
        if {f'{milestone}.{i}' for i in range(1, count + 1)} - set(ids):
            raise ValueError(f'Missing acceptance checklist coverage: {milestone}')
    blocking = []
    for provider in sorted(targets):
        for scenario in ['natural-desktop', 'desktop-crash']:
            identifier = f'{provider}.{scenario}'
            matched = [entry for entry in entries if entry['id'] == identifier]
            if len(matched) != 1 or matched[0]['status'] != 'passed':
                blocking.append(identifier)
    blocking += [entry['id'] for entry in entries if entry['id'].startswith(('m2b.', 'm2c.', 'm2d.', 'm2e.')) and entry['status'] != 'passed']
    for provider, paths in manifest['liveProofs'].items():
        if provider not in targets:
            raise ValueError('Unknown live provider')
        for kind, name in paths.items():
            if name not in manifest['sha256']:
                raise ValueError('Unpinned live proof')
            proof = json.loads((root / name).read_text())
            if proof['provider'] != provider:
                raise ValueError('Provider attribution mismatch')
            if kind == 'natural-desktop':
                if proof['proof']['status'] != 'passed' or proof['proof']['gateCount'] < 1:
                    raise ValueError('Natural desktop did not pass independent gates')
            elif kind == 'desktop-crash':
                if proof['status'] != 'passed' or proof['hostExit'] != -9 or not all(proof[key] for key in ['noNewDispatchReceipts', 'processesReaped', 'originalCheckoutPreserved']):
                    raise ValueError('Incomplete crash evidence')
                before = proof['before']['snapshot']
                recovery = proof['recovery']
                after = recovery['snapshot']
                if recovery['status'] != 'passed' or recovery['step'] != 'crash-recovered-blocked':
                    raise ValueError('No successful cold recovery observation')
                active = {task for task, state in before['taskStates'].items() if state == 'RUNNING'}
                if not active or before.get('completionReceipts'):
                    raise ValueError('Invalid crash point: no live task or already completed')
                if any(after['taskStates'].get(task) != ('BLOCKED' if task in active else state) for task, state in before['taskStates'].items()):
                    raise ValueError('Recovery advanced an uncertain task or downstream')
                if set(recovery['evidence']) != active or before['taskTurnIds'] != after['taskTurnIds'] or any(not value['runtimeIdle'] or not value['launchRecorded'] for value in recovery['evidence'].values()):
                    raise ValueError('Attempt changed or native idle/launch evidence missing')
                if (after.get('delivery') or {}).get('phase') in {'ready', 'merged'}:
                    raise ValueError('Uncertain recovery allowed delivery')
            else:
                raise ValueError('Unknown live proof kind')
    for provider in targets:
        for kind in ['natural-desktop', 'desktop-crash']:
            if f'{provider}.{kind}' not in blocking and kind not in manifest['liveProofs'].get(provider, {}):
                raise ValueError('Passed scenario has no machine-readable live proof')
    by_id = {entry['id']: entry for entry in entries}
    for entry in entries:
        if entry['status'] == 'failed' and not (entry.get('resolvedBy') and all(by_id.get(identifier, {}).get('status') == 'passed' for identifier in entry['resolvedBy'])):
            blocking.append(entry['id'])
    complete = not blocking
    if manifest['complete'] != complete:
        raise ValueError('Declared milestone result differs from mandatory live evidence')
    if require_complete and not complete:
        raise ValueError(f'M2 remains incomplete: {blocking}')
    counts = {status: sum(e['status'] == status for e in entries) for status in ['passed', 'failed', 'untested']}
    print(json.dumps(dict(integrity='passed', complete=complete, counts=counts, blocking=blocking), ensure_ascii=False))
    return manifest


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-complete', action='store_true')
    args = parser.parse_args()
    verify(require_complete=args.require_complete)
