#!/usr/bin/env python3
"""Offline native result fixture; never imported by product code."""
import json
from pathlib import Path
import sys

if '--version' in sys.argv:
    print('2.1.288 (Claude Code)')
elif sys.argv[1:3] == ['auth', 'status']:
    print(json.dumps({'loggedIn': True, 'authMethod': 'claude.ai'}))
else:
    sys.stdin.read()
    assert sys.argv[sys.argv.index('--output-format') + 1] == 'json'
    schema = json.loads(sys.argv[sys.argv.index('--json-schema') + 1])
    assert schema['required'] == ['verdict', 'findings']
    assert schema['additionalProperties'] is False
    mode = Path(__file__).with_name('structured-mode.txt').read_text().strip()
    result = {'type': 'result', 'subtype': 'success', 'is_error': False,
              'result': 'Conversational PASS is never authoritative.',
              'usage': {'input_tokens': 10, 'output_tokens': 3}, 'total_cost_usd': 0.01}
    if mode != 'missing':
        result['structured_output'] = {'verdict': 'FAIL' if mode == 'fail' else 'PASS',
                                       'findings': ['actual issue'] if mode == 'fail' else []}
    print(json.dumps(result))
