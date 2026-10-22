#!/usr/bin/env python3
"""Offline vendor-protocol fixture. Never imported by application code."""
import json
from pathlib import Path
import sys
import time

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
if mode == 'stream-hang':
    for _ in range(10000):
        emit({'type': 'item.completed', 'item': {'id': 'progress', 'type': 'agent_message', 'text': 'Working'}})
        time.sleep(.003)
if mode == 'hang':
    time.sleep(60)
if mode == 'nonzero':
    emit({'type': 'turn.failed', 'error': {'message': 'offline failure'}})
    raise SystemExit(7)
if '# Tokage Task Brief v1' in prompt:
    target = Path('unexpected.txt' if mode == 'out-of-scope' else 'hello.txt')
    target.write_text('wrong\n' if mode == 'gate-failure' else 'after\n')
    emit({'type': 'item.completed', 'item': {'id': 'edit', 'type': 'file_change', 'status': 'completed', 'changes': [{'path': str(target.resolve()), 'kind': 'update'}]}})
    text = '### What was completed\nUpdated greeting\n### Deviations from contracts\nNone\n### Hints for downstream\nNone\n### Self-check against acceptance criteria\nGreeting updated'
    if mode == 'deviation':
        text = text.replace('### Deviations from contracts\nNone', '### Deviations from contracts\nChanged the contract')
elif 'Read-only delivery review' in prompt:
    text = json.dumps({'verdict': 'FAIL' if mode == 'review-failure' else 'PASS', 'findings': ['offline finding'] if mode == 'review-failure' else []})
    if mode == 'review-write':
        Path('hello.txt').write_text('tampered\n')
else:
    text = (root / 'plan.md').read_text()
emit({'type': 'item.completed', 'item': {'id': 'reply', 'type': 'agent_message', 'text': text}})
if mode != 'missing-done':
    emit({'type': 'turn.completed', 'usage': {'input_tokens': 10, 'output_tokens': 5}})
