#!/usr/bin/env python3
"""Read-only replay of real M2.e records; never touches the running application DB."""
import argparse
import hashlib
import json
import math
import sqlite3
from pathlib import Path


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def read_db(path):
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    return db


def turn_cost(p, context, warm):
    weighted = context * (p['cacheReadMultiplier'] if warm else p['cacheWriteMultiplier'])
    weighted += (max(1, p['apiCallsPerTurn']) - 1) * context * p['cacheReadMultiplier']
    weighted += p['growthPerTurn'] * p['cacheWriteMultiplier']
    return (weighted * p['pIn'] + p['outputPerTurn'] * p['pOut']) / 1_000_000


def forecast(p, warm):
    b, hst, count = p['baselineTokens'], p['historyTokens'], max(1, p['remainingTurns'])
    summary = hst * p['compactOutputRatio']
    history_read = (b + hst) * (p['cacheReadMultiplier'] if warm else p['cacheWriteMultiplier']) * p['pIn'] / 1_000_000
    rework = hst * p['pIn'] * p['cacheWriteMultiplier'] / 1_000_000
    cont = sum(turn_cost(p, b + hst + i*p['growthPerTurn'], warm if i == 0 else True) for i in range(count))
    compact = history_read + summary*p['pOut']/1_000_000 + .1*rework
    compact += sum(turn_cost(p, b + summary + i*p['growthPerTurn'], True) for i in range(count))
    fresh = history_read + 300*p['pOut']/1_000_000 + .3*rework
    fresh += sum(turn_cost(p, b + 3300 + i*p['growthPerTurn'], i > 0) for i in range(count))
    return {'continueCostUsd': cont, 'compactCostUsd': compact, 'freshCostUsd': fresh}


def replay_queue(db, action_id, pack):
    row = dict(db.execute('SELECT * FROM context_actions WHERE id=?', [action_id]).fetchone())
    meta = json.loads(row['est_costs_json'])
    intent, preview = meta['queuedIntent'], meta['preview']
    scope = intent['scope']
    messages = []
    for raw in db.execute("SELECT meta_json FROM chat_logs WHERE main_session_id=? AND role='ASSISTANT' ORDER BY seq", [scope['sessionId']]):
        m = json.loads(raw[0])
        if m.get('nativeThreadId') == scope['window'] and m.get('status') == 'done' and m.get('model') == scope['model']:
            messages.append(m)
        if m.get('id') == scope['turnId']:
            break
    assert len(messages) >= 2
    totals = [m['usage']['contextTokens'] for m in messages]
    growth = sum(max(0, b-a) for a,b in zip(totals, totals[1:])) / (len(totals)-1)
    definition = pack['models'][scope['model']]
    provider = pack['providers'][scope['providerName']]
    prices = definition['price_per_mtok'].copy()
    tier = definition.get('long_context_pricing')
    if tier and totals[-1] > tier['threshold']:
        prices['input'] *= tier['input_multiplier']; prices['output'] *= tier['output_multiplier']
    p = dict(baselineTokens=8000, historyTokens=totals[-1]-8000,
             remainingTurns=max(1,4-len(messages)), apiCallsPerTurn=math.ceil(sum(m['usage']['apiCalls'] for m in messages)/len(messages)),
             growthPerTurn=growth, outputPerTurn=sum(m['usage']['output'] for m in messages)/len(messages),
             compactOutputRatio=provider['context']['compact_output_ratio'], cacheReadMultiplier=definition['cache_rates']['read_multiplier'],
             cacheWriteMultiplier=definition['cache_rates']['write_multiplier'], pIn=prices['input'], pOut=prices['output'])
    # These queue snapshots are the original cache-aware-ON scenarios only.
    costs = forecast(p, preview['warmNow'])
    for key, value in costs.items():
        assert abs(value-preview[key]) < 1e-10, (action_id, key, value, preview[key])
    assert min(('COMPACT','FRESH'),key=lambda kind:costs['compactCostUsd' if kind=='COMPACT' else 'freshCostUsd']) == preview['recommendedAction']
    return {'actionId':action_id, 'model':scope['model'], 'params':p, 'costs':costs, 'recommendation':preview['recommendedAction'], 'matchesStoredPreview':True}


def verify_carrier(db, action_id, old_thread, user_path, root, native_root):
    row = dict(db.execute('SELECT * FROM context_actions WHERE id=?', [action_id]).fetchone())
    assert row['status'] == 'APPLIED' and row['new_attempt_id']
    raw = db.execute('SELECT meta_json FROM chat_logs WHERE message_id=?',[row['new_attempt_id']]).fetchone()
    message = json.loads(raw[0]); assert message['status'] == 'done'
    prefix = message['compactAction']['preparedPayload']
    thread = message['nativeThreadId']; assert thread != old_thread
    expected = prefix+'\n\n---\n\n'+user_path.read_text().strip()
    provenance = json.loads(prefix.splitlines()[-1])['progressReport']
    assert provenance['source'] == 'LOCAL_EXTRACTION'
    assert provenance['sourceTurnIds'][-1] == row['attempt_id']
    assert len(set(provenance['sourceTurnIds'])) == len(provenance['sourceTurnIds'])
    paths = list(native_root.rglob(f'*{thread}*.jsonl')); assert len(paths)==1
    task_inputs=[]
    for line in paths[0].open():
        record=json.loads(line); payload=record.get('payload',{})
        if record.get('type')!='response_item' or payload.get('role')!='user': continue
        for part in payload.get('content',[]):
            text=part.get('text','')
            if text.startswith('# New-window retained brief'): task_inputs.append(text)
    assert task_inputs==[expected]
    (root/f'action-{action_id}-native-first-task.txt').write_text(expected)
    event=dict(db.execute("SELECT * FROM savings_events WHERE ref_id=?",[f'context_action:{action_id}']).fetchone())
    detail=json.loads(event['detail_json']); spec=json.loads(row['est_costs_json'])['settlementV1'];p=spec['params']
    assert detail['basis']=='first_turn_counterfactual_v1' and detail['horizonTurns']==1 and detail['method']=='fresh'
    warm=p['cacheType']!='none' and p['elapsedSecondsSinceLastTurn']<p['cacheTtlSeconds']
    baseline=forecast(p,warm)['continueCostUsd']
    assert abs(baseline-event['baseline_usd'])<1e-10
    assert message['usage'].get('costUsd') is None and event['actual_usd'] is None and not event['settled'] and row['actual_saving_usd'] is None
    if row['kind']=='FRESH': assert p['cacheReadMultiplier']==p['cacheWriteMultiplier']==1 and p['cacheType']=='none'
    return {'actionId':action_id,'kind':row['kind'],'trigger':row['trigger'],'sourceTurnId':row['attempt_id'],'targetTurnId':row['new_attempt_id'],
            'oldThread':old_thread,'newThread':thread,'firstTaskBytes':len(expected.encode()),'firstTaskSha256':digest(expected),'nativeFirstTaskMatchesExactly':True,
            'nativeFile':str(paths[0]),'progressSource':provenance['source'],'sourceTurnIds':provenance['sourceTurnIds'],
            'reportedUsage':message['usage'],'baselineFirstTurnUsd':baseline,'actualUsd':None,'settled':False,'unknownIsNotZero':True,'reason':detail['reason']}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence-root',type=Path,required=True)
    parser.add_argument('--source-root',type=Path,required=True)
    parser.add_argument('--native-root',type=Path,default=Path.home()/'.codex/sessions')
    args=parser.parse_args();root=args.evidence_root
    db=read_db(root/'real-acceptance-final.sqlite'); pack=json.loads((args.source_root/'src-tauri/resources/knowledge/builtin.json').read_text())
    for aid in (1,3):
        row=db.execute('SELECT status,new_attempt_id,applied_at FROM context_actions WHERE id=?',[aid]).fetchone()
        assert tuple(row)==('UNDONE',None,None)
    actions=[dict(r) for r in db.execute('SELECT id,kind,trigger,status,new_attempt_id FROM context_actions ORDER BY id')]
    assert len(actions)==4
    replays=[replay_queue(read_db(root/name),aid,pack) for aid,name in [(1,'codex-auto-queued.sqlite'),(2,'codex-before-synth-carrier.sqlite'),(3,'luna56-cache-on-stopped.sqlite')]]
    carriers=[verify_carrier(db,2,'01a18a13-c0ae-75f1-958b-a56487039f81',root/'materials/codex-prompt-4.txt',root,args.native_root),
              verify_carrier(db,4,'01a18a1c-82e1-7921-a1cf-3aef2a49d448',root/'materials/fresh-carrier.txt',root,args.native_root)]
    usage=[dict(r) for r in db.execute('SELECT provider_name,model_id,input_tokens,output_tokens,api_calls,context_tokens,cost_usd,equiv_cost_usd,phase,source FROM usage_events ORDER BY id')]
    assert len(usage)==7 and all(u['provider_name']=='CODEX_CLI' and u['cost_usd'] is None and u['source']=='STRUCTURED' for u in usage)
    meta=[json.loads(r[0]) for r in db.execute("SELECT meta_json FROM chat_logs WHERE role='ASSISTANT'")]
    assert not any(m.get('status') in ('streaming','waiting_approval') for m in meta)
    result={'status':'scoped-pass-with-native-account-block','runtimeSourceCommit':'fbdac3aaa66b4b3a5ad81c5a381cccc8e986a2f2',
            'actions':actions,'queueCostReplays':replays,'carriers':carriers,'usage':usage,
            'codexReportedDollarCost':None,'codexPackEquivalentUsd':sum(u['equiv_cost_usd'] or 0 for u in usage),
            'M2e1':'PASS: auto queue, normal undo, exact same-thread unprefixed follow-up, later auto applied carrier',
            'M2e2':'PASS scoped to genuine FRESH-kind, cache-aware disabled by normal UI, local progress, exact first task and unknown-cost ledger',
            'M2e3':'PARTIAL: Codex unsupported-native fallback passes; Claude native blocked by account hold before compact',
            'M2c2c3':'BLOCKED: Claude2.1.288 first normal Haiku request returned account restriction; no workaround or retry',
            'limits':['Not vendor-cache-miss evidence','No positive or actual-dollar savings claim','No warm report generation/real native compact acceptance','Ephemeral WebView; no persistence across restart certification','Review model claims remain workload output, not independently accepted defects']}
    (root/'verification.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({'result':result['status'],'actions':len(actions),'carriers':len(carriers),'codexCompletedTurns':len(usage),'packEquivalentUsdNotActual':result['codexPackEquivalentUsd'],'nativeAccountBlocked':True}))


if __name__=='__main__': main()
