#!/usr/bin/env python3
"""Offline vendor-protocol fixture. Never imported by application code."""
import json
from pathlib import Path
import sys
import time
import re
import os

root = Path(__file__).resolve().parent
if '--version' in sys.argv:
    print('codex-cli 0.162.0')
    raise SystemExit()
if sys.argv[1:3] == ['login', 'status']:
    print('Logged in using ChatGPT', file=sys.stderr)
    raise SystemExit()
if 'exec' not in sys.argv:
    raise SystemExit()
prompt = sys.stdin.read()
mode = (root / 'mode.txt').read_text().strip()
with (root / 'calls.jsonl').open('a') as f:
    f.write(json.dumps({'args': sys.argv[1:], 'cwd': str(Path.cwd())}) + '\n')

def emit(event):
    print(json.dumps(event), flush=True)

emit({'type': 'thread.started', 'thread_id': 'offline-fixture-thread'})
if mode == 'hang':
    time.sleep(60)
if mode == 'nonzero':
    emit({'type': 'turn.failed', 'error': {'message': 'offline failure'}})
    raise SystemExit(7)
if '# Tokage Task Brief v1' in prompt:
    scope = re.search(r'^- Write scope: (.+)$', prompt, re.M).group(1).split(',')[0].strip().removesuffix('/**')
    task = re.search(r'^- ID: (\S+)', prompt, re.M).group(1)
    with (root / 'timeline.jsonl').open('a') as f:
        f.write(json.dumps({'kind': 'start', 'task': task, 'time': time.monotonic(), 'cwd': str(Path.cwd())}) + '\n')
    if mode == 'parallel':
        (root / ('active-' + task)).touch()
        stop = time.monotonic() + 5
        while len(list(root.glob('active-*'))) < 2 and time.monotonic() < stop:
            time.sleep(.01)
    target = Path('unexpected.txt' if mode == 'out-of-scope' else scope)
    target.parent.mkdir(parents=True, exist_ok=True)
    counter = root / ('attempts-' + task)
    number = int(counter.read_text()) + 1 if counter.exists() else 1
    counter.write_text(str(number))
    target.write_text('wrong\n' if mode == 'gate-failure' or mode == 'repair-once' and number == 1 else 'after\n')
    with (root / 'timeline.jsonl').open('a') as f:
        f.write(json.dumps({'kind': 'end', 'task': task, 'time': time.monotonic()}) + '\n')
    emit({'type': 'item.completed', 'item': {'id': 'edit', 'type': 'file_change', 'status': 'completed', 'changes': [{'path': str(target.resolve()), 'kind': 'update'}]}})
    text = '### What was completed\nUpdated greeting\n### Deviations from contracts\nNone\n### Hints for downstream\nNone\n### Self-check against acceptance criteria\nGreeting updated'
    if mode == 'deviation':
        text = text.replace('### Deviations from contracts\nNone', '### Deviations from contracts\nChanged the contract')
elif 'Read-only delivery review' in prompt:
    (root / 'review-prompt.txt').write_text(prompt)
    text = json.dumps({'verdict': 'FAIL' if mode == 'review-failure' else 'PASS', 'findings': ['offline finding'] if mode == 'review-failure' else []})
    if mode == 'review-write':
        Path('hello.txt').write_text('tampered\n')
else:
    text = (root / 'plan.md').read_text()
emit({'type': 'item.completed', 'item': {'id': 'reply', 'type': 'agent_message', 'text': text}})
if mode != 'missing-done':
    emit({'type': 'turn.completed', 'usage': {'input_tokens': 10, 'output_tokens': 5}})
