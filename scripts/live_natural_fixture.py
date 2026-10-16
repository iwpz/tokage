"""Independent m2b requirements/oracle. Never generates or injects a task graph."""
import json
import subprocess

FILES = {
    'pricing.mjs': 'export function subtotal(items) { return 0; }\n',
    'format.mjs': 'export function money(cents) { return "TODO"; }\n',
    'invoice.mjs': 'export function invoice(items) { return "TODO"; }\n',
}
ORACLE = '''import assert from 'node:assert/strict';
const part = process.argv[2];
if (part === 'pricing' || part === 'all') {
  const {subtotal} = await import('./pricing.mjs');
  assert.equal(subtotal([]), 0);
  assert.equal(subtotal([{priceCents:125, quantity:2},{priceCents:50, quantity:3}]), 400);
  assert.equal(subtotal([{priceCents:0, quantity:9}]), 0);
  for (const item of [{priceCents:-1,quantity:1},{priceCents:1.5,quantity:1},{priceCents:1,quantity:-1},{priceCents:1,quantity:0.5}]) assert.throws(() => subtotal([item]), RangeError);
}
if (part === 'format' || part === 'all') {
  const {money} = await import('./format.mjs');
  for (const [n,s] of [[0,'$0.00'],[1,'$0.01'],[105,'$1.05'],[-105,'-$1.05'],[120000,'$1200.00']]) assert.equal(money(n),s);
}
if (part === 'integration' || part === 'all') {
  const {invoice} = await import('./invoice.mjs');
  assert.equal(invoice([]), 'Total: $0.00');
  assert.equal(invoice([{priceCents:125,quantity:2},{priceCents:50,quantity:3}]), 'Total: $4.00');
  assert.throws(() => invoice([{priceCents:-1,quantity:2}]), RangeError);
}
assert.ok(['pricing','format','integration','all'].includes(part));
console.log('TOKAGE_NATURAL_' + part.toUpperCase() + '_OK');
'''
REQUEST = '''Please plan and implement this multi-module invoice feature in this existing Node ES module repository. Review the code before planning; produce the Tokage plan, not implementation during planning.
subtotal(items) in pricing.mjs must sum integer priceCents * integer quantity, return 0 for empty input and throw RangeError for negative or non-integer prices/quantities. Zero values are valid. money(cents) in format.mjs must produce dollar strings with exactly two digits, no thousands separator, and a leading minus before the dollar sign for negatives (e.g. -105 => -$1.05). invoice(items) in invoice.mjs must compose imports of those two public functions and return "Total: " plus the formatted subtotal, propagating errors.
Arrange independent pricing and formatting work so it can run in parallel, followed by integration that consumes both results. Record their cross-module public contracts. The environment is prepared with npm ci before execution. Use npm run test:pricing, npm run test:format, npm run test:integration for the respective work, and include npm test on the integration task for the final full gate. Do not edit tests, package files, configuration or add dependencies. Only pricing.mjs, format.mjs, invoice.mjs may change. Execution agents should use file tools only; Tokage runs shell acceptance independently. Preserve public exports and use the required TaskReport format after execution. No model fallback.'''

def populate(repo):
    files = dict(FILES)
    files['check.mjs'] = ORACLE
    files['package.json'] = json.dumps({'name':'tokage-natural-acceptance','version':'1.0.0','private':True,'type':'module','scripts':{'test':'node check.mjs all', **{f'test:{p}':f'node check.mjs {p}' for p in ['pricing','format','integration']}}}, indent=2) + '\n'
    for name, content in files.items(): (repo / name).write_text(content)
    subprocess.run(['npm','install','--package-lock-only','--ignore-scripts','--no-audit','--no-fund'], cwd=repo, check=True, capture_output=True)
    files['package-lock.json'] = (repo / 'package-lock.json').read_text()
    return files

def prepare(root, repo):
    run = subprocess.run(['npm','ci','--ignore-scripts','--no-audit','--no-fund'],cwd=repo,capture_output=True,text=True)
    (root/'environment.json').write_text(json.dumps({'command':'npm ci --ignore-scripts --no-audit --no-fund','exitCode':run.returncode,'output':run.stdout+run.stderr},indent=2))
    run.check_returncode()
    # Baseline defects must independently fail, including each parallel unit.
    for part in ['pricing','format','integration']:
        run = subprocess.run(['node','check.mjs',part],cwd=repo,capture_output=True,text=True)
        assert run.returncode != 0, f'Oracle failed to reject seeded {part} defect'
    (root/'original-request.txt').write_text(REQUEST)
    (root/'independent-oracle.mjs').write_text(ORACLE)

def verify(root, repo, original, result):
    snapshot = result['snapshot']
    tasks = snapshot['tasks']
    ids = {t['id'] for t in tasks}
    assert len(ids) >= 3
    assert set(snapshot['taskStates']) == ids and set(snapshot['taskStates'].values()) == {'DONE'}
    assert result['reviewedPlan']['contracts'], 'Missing cross-module contracts'
    assert result['planning']['mode'] == 'plan' and result['planning']['status'] == 'done'
    assert result['planning']['provider'] == result['provider']
    assert result['planning']['model'] == result['model']
    assert result['planning']['text'] and result['planning']['plan']
    assert result['originalRequest'] == REQUEST
    # Confirm actual overlapping execution and a later join, not just a drawn DAG.
    starts, ends = snapshot['taskStartedAt'], snapshot['taskEndedAt']
    pairs = [(a,b) for a in tasks for b in tasks if a['id'] < b['id'] and not a['depends_on'] and not b['depends_on'] and max(starts[a['id']], starts[b['id']]) < min(ends[a['id']], ends[b['id']])]
    assert pairs, 'No actual parallel execution observed'
    assert any(len(t['depends_on']) >= 2 and all(starts[t['id']] >= ends[d] for d in t['depends_on']) for t in tasks), 'No ordered integration join'
    for name, content in original.items():
        if name not in FILES: assert (repo/name).read_text() == content, f'Acceptance weakened: {name}'
    assert (root/'independent-oracle.mjs').read_text() == ORACLE
    # Use our separate original oracle copy, with imports bound to delivered modules.
    oracle = ORACLE
    for name in FILES: oracle = oracle.replace("'./"+name+"'", json.dumps((repo/name).as_uri()))
    (root/'delivered-oracle.mjs').write_text(oracle)
    run = subprocess.run(['node',str(root/'delivered-oracle.mjs'),'all'],capture_output=True,text=True)
    (root/'independent-result.json').write_text(json.dumps({'exitCode':run.returncode,'output':run.stdout+run.stderr},indent=2))
    run.check_returncode()
    # Integration must really import both modules, not hard-code fixture answers.
    source = (repo/'invoice.mjs').read_text()
    assert './pricing.mjs' in source and './format.mjs' in source
    assert any(g['command'] == 'npm test' and g['passed'] for g in snapshot['gateResults'])
    return ids
