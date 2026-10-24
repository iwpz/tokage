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
if 'app-server' in sys.argv:
    def emit_rpc(value):
        print(json.dumps(value), flush=True)
    mode = (root / 'mode.txt').read_text().strip()
    for line in sys.stdin:
        request = json.loads(line)
        method = request.get('method')
        if method == 'initialize':
            emit_rpc({'id': request['id'], 'result': {'userAgent': 'offline fixture'}})
        elif method in ('thread/start', 'thread/resume'):
            emit_rpc({'id': request['id'], 'result': {'thread': {'id': 'offline-fixture-thread'}, 'approvalPolicy': 'never', 'sandbox': {'type': 'readOnly'}}})
        elif method == 'turn/start':
            with (root / 'calls.jsonl').open('a') as f:
                f.write(json.dumps({'args': sys.argv[1:], 'cwd': str(Path.cwd()), 'params': request['params']}) + '\n')
            base = {'threadId': 'offline-fixture-thread', 'turnId': 'fixture-turn'}
            emit_rpc({'id': request['id'], 'result': {'turn': {'id': 'fixture-turn', 'status': 'inProgress'}}})
            if mode == 'hang':
                time.sleep(60)
            if mode == 'stream-hang':
                for _ in range(10000):
                    emit_rpc({'method': 'item/agentMessage/delta', 'params': dict(base, itemId='reply', delta='Working')})
                    time.sleep(.003)
            if mode == 'crash-wait':
                deadline = time.monotonic() + 30
                while not (root / 'release').exists() and time.monotonic() < deadline:
                    time.sleep(.02)
            if mode == 'nonzero':
                emit_rpc({'method': 'turn/completed', 'params': {'threadId': base['threadId'], 'turn': {'id': base['turnId'], 'status': 'failed', 'error': {'message': 'offline failure'}}}})
                raise SystemExit(7)
            if mode == 'approval':
                emit_rpc({'id': 100, 'method': 'item/commandExecution/requestApproval', 'params': dict(base, command='$CMD ls')})
                continue
            text = (root / 'plan.md').read_text()
            for start in range(0, len(text), 32):
                emit_rpc({'method': 'item/agentMessage/delta', 'params': dict(base, itemId='reply', delta=text[start:start + 32])})
            emit_rpc({'method': 'item/completed', 'params': dict(base, item={'type': 'agentMessage', 'id': 'reply', 'text': 'changed' if mode == 'changed-aggregate' else text})})
            emit_rpc({'method': 'thread/tokenUsage/updated', 'params': dict(base, tokenUsage={'last': {'inputTokens': 10, 'outputTokens': 5, 'cachedInputTokens': 0}})})
            if mode != 'missing-done':
                emit_rpc({'method': 'turn/completed', 'params': {'threadId': base['threadId'], 'turn': {'id': base['turnId'], 'status': 'completed'}}})
            else:
                raise SystemExit()
            if mode == 'success-nonzero':
                raise SystemExit(7)
            if mode == 'late-error':
                emit_rpc({'method': 'error', 'params': dict(base, error={'message': 'late protocol failure'})})
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
if mode == 'approval':
    emit({'type': 'item.started', 'item': {'type': 'command_execution', 'id': 'blocked-command', 'command': '$CMD ls'}})
    emit({'type': 'item.completed', 'item': {'type': 'command_execution', 'id': 'blocked-command', 'command': '$CMD ls', 'exit_code': 1, 'aggregated_output': 'permission denied'}})
    time.sleep(.3)
    raise SystemExit()
if mode == 'stream-hang':
    for _ in range(10000):
        emit({'type': 'item.completed', 'item': {'id': 'progress', 'type': 'agent_message', 'text': 'Working'}})
        time.sleep(.003)
if mode == 'crash-wait':
    deadline = time.monotonic() + 30
    while not (root / 'release').exists() and time.monotonic() < deadline:
        time.sleep(.02)
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
# Match the normal vendor output contract; prose alone is never native evidence.
if '--output-schema' in sys.argv and mode != 'review-missing-native':
    schema = json.loads(Path(sys.argv[sys.argv.index('--output-schema') + 1]).read_text())
    assert schema['type'] == 'object'
    output = Path(sys.argv[sys.argv.index('--output-last-message') + 1])
    output.write_text(json.dumps(json.loads(text)))
emit({'type': 'item.completed', 'item': {'id': 'reply', 'type': 'agent_message', 'text': text}})
if mode != 'missing-done':
    emit({'type': 'turn.completed', 'usage': {'input_tokens': 10, 'output_tokens': 5}})
