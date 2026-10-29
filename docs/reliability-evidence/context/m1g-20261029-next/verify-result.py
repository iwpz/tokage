#!/usr/bin/env python3
"""Read-only independent verification of this recorded main-session carrier."""
import argparse
import hashlib
import json
import sqlite3
from pathlib import Path


def verify(root):
    conn = sqlite3.connect(f'file:{root / "carrier-complete.sqlite"}?mode=ro', uri=True)
    conn.row_factory = sqlite3.Row
    messages = [json.loads(row[0]) for row in conn.execute(
        "SELECT meta_json FROM chat_logs WHERE role='ASSISTANT' ORDER BY id")]
    assert len(messages) == 4
    assert all(m['status'] == 'done' and m['provider'] == 'AIDER'
               and m['reportedModel'] == 'deepseek/deepseek-flash'
               and m['usage']['source'] == 'STDOUT_REGEX'
               and m['usage']['costUsd'] is not None for m in messages)
    carrier = messages[-1]
    assert carrier['compactAction']['state'] == 'completed'
    old_threads = {m['nativeThreadId'] for m in messages[:-1]}
    assert len(old_threads) == 1 and carrier['nativeThreadId'] not in old_threads
    history = (root / 'carrier-cli-history.md').read_text()
    lines = history.splitlines()
    decoded = '\n'.join(line[5:-2] if line.endswith('  ') else line[5:]
                        for line in lines if line.startswith('#### '))
    payload = carrier['compactAction']['preparedPayload']
    expected = payload + '\n\n---\n\n' + (root / 'prompt-4-carrier.txt').read_text()
    assert decoded == expected
    assert json.loads(payload.split('\n')[-1])['taskGoal'] == (root / 'prompt-1.txt').read_text()
    argv = next(line[2:].split() for line in lines
                if line.startswith('> ') and '--message-file' in line)
    assert '--message-file' in argv and '--no-restore-chat-history' in argv
    assert '--restore-chat-history' not in argv
    assert sum(line.startswith('#### ') and (i == 0 or not lines[i-1].startswith('#### '))
               for i, line in enumerate(lines)) == 1
    actions = list(conn.execute('SELECT * FROM context_actions'))
    events = list(conn.execute("SELECT * FROM savings_events WHERE kind='CONTEXT'"))
    assert len(actions) == len(events) == 1
    action, event = actions[0], events[0]
    spec = json.loads(action['est_costs_json'])['settlementV1']
    detail = json.loads(event['detail_json'])
    p = spec['params']
    assert p['remainingTurns'] == spec['horizonTurns'] == detail['horizonTurns'] == 1
    assert p['apiCallsPerTurn'] == 1 and p['cacheType'] == 'disk_persistent'
    # One warm call for the same first-turn horizon, calculated independently.
    baseline = (p['pIn'] / 1e6 * ((p['baselineTokens'] + p['historyTokens']) *
                p['cacheReadMultiplier'] + p['growthPerTurn'] * p['cacheWriteMultiplier']) +
                p['pOut'] / 1e6 * p['outputPerTurn'])
    cost = carrier['usage']['costUsd']
    assert action['status'] == 'APPLIED' and action['kind'] == 'COMPACT'
    assert action['id'] == carrier['compactAction']['actionId'] == detail['actionId']
    assert action['new_attempt_id'] == carrier['id'] == detail['targetId']
    assert event['ref_id'] == f"context_action:{action['id']}"
    assert detail['basis'] == 'first_turn_counterfactual_v1' and detail['method'] == 'fresh'
    assert event['settled'] == 1
    assert abs(event['baseline_usd'] - baseline) < 1e-12
    assert event['actual_usd'] == detail['observedCarrierCostUsd'] == cost
    assert action['actual_saving_usd'] == max(baseline - cost, 0)
    return {'verified': True, 'realCompletedTurns': len(messages),
            'reportedTotalCostUsd': round(sum(m['usage']['costUsd'] for m in messages), 8),
            'carrierPayloadSha256': hashlib.sha256(decoded.encode()).hexdigest(),
            'baselineUsd': baseline, 'reportedCarrierCostUsd': cost,
            'settledCounterfactualSavingUsd': action['actual_saving_usd']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('evidence_root', type=Path, help='External evidence directory with original snapshot/history')
    args = parser.parse_args()
    print(json.dumps(verify(args.evidence_root.resolve()), indent=2))
