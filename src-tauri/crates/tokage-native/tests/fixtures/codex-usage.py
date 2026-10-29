#!/usr/bin/python3
"""Offline management-protocol fixture; never imported by production."""
import json, os, sys, time
from pathlib import Path
root = Path(__file__).parent
mode = (root / 'mode').read_text().strip()
(root / 'pid').write_text(str(os.getpid()))
(root / 'argv').write_text(json.dumps(sys.argv[1:]))
def send(v):
    print(json.dumps(v), flush=True)
account_reads = 0
for line in sys.stdin:
    v = json.loads(line)
    method = v.get('method')
    with (root / 'requests').open('a') as f:
        f.write(json.dumps(v) + '\n')
    if method == 'initialized':
        continue
    if method == 'initialize':
        result = {}
    elif method == 'account/read':
        account_reads += 1
        result = {'account': {'type': 'apiKey' if mode == 'apikey' else 'chatgpt', 'email': 'synthetic@example.invalid', 'planType': 'plus'}, 'requiresOpenaiAuth': True}
        if account_reads > 1:
            if mode == 'plan-change':
                result['account']['planType'] = 'pro'
            elif mode == 'email-change':
                result['account']['email'] = 'different@example.invalid'
            elif mode == 'auth-type-change':
                result['account']['type'] = 'apiKey'
    elif method == 'account/rateLimits/read':
        if mode == 'hang':
            time.sleep(60)
        if mode == 'server-request':
            send({'id': 100, 'method': 'account/chatgptAuthTokens/refresh', 'params': {}})
            continue
        if mode in ('401', '429', 'unsupported'):
            send({'id': v['id'], 'error': {'code': -32601 if mode == 'unsupported' else -32000, 'message': {'401':'401 Unauthorized private-secret', '429':'429 rate limit private-secret', 'unsupported':'not supported'}[mode]}})
            continue
        if mode in ('refresh', 'switch'):
            path = Path(os.environ['CODEX_HOME']) / 'auth.json'
            a = json.loads(path.read_text())
            a['tokens']['refresh_token'] = 'rotated-synthetic'
            if mode == 'switch':
                a['tokens']['account_id'] = 'different-workspace'
            path.write_text(json.dumps(a))
        result = {'accountId': 'different-workspace' if mode == 'mismatch' else 'synthetic-workspace', 'rateLimits': {'primary': {'usedPercent': 12.5, 'windowDurationMins': 300, 'resetsAt': 2000000000}}}
        if mode == 'missing-account':
            result.pop('accountId')
        elif mode == 'null-account':
            result['accountId'] = None
    else:
        (root / 'unexpected').write_text(str(method))
        sys.exit(1)
    send({'id': v['id'], 'result': result})
