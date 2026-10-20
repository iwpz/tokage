#!/usr/bin/env python3
"""Dispatch a preregistered real Tokage requirement through the desktop pipeline.
No business-code edits, model fallback, gate rewriting or automatic review approval.
Writes an isolated clone, execution evidence and waits for independent-review.json.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import shutil
import time
from runpy import run_path

args_parser = argparse.ArgumentParser(description=__doc__)
args_parser.add_argument('--source-repo', type=Path, help='Preserved candidate checkout for an in-scope retry')
args_parser.add_argument('--scroll-probe-repo', type=Path, help='Read-only real-stream validation of an already delivered checkout')
args_parser.add_argument('--feedback-file', type=Path, help='Preserved review findings for a retry of the same requirement')
args_parser.add_argument('--npm-cache-template', type=Path, help='Preverified public dependency cache copied into each isolated task workspace')
args = args_parser.parse_args()
BASE = Path(__file__).resolve().parents[1]
git = run_path(str(BASE / 'scripts/run-live-desktop.py'))['git']
root = Path(tempfile.mkdtemp(prefix='tokage-live-desktop-m3a-', dir='/tmp')).resolve()
repo = root / 'repo'
subprocess.run(['git', 'clone', '--no-hardlinks', str((args.scroll_probe_repo or args.source_repo or BASE).resolve()), str(repo)], check=True)
git(repo, 'config', 'user.name', 'Tokage m3a')
git(repo, 'config', 'user.email', 'm3a@tokage.invalid')
(root / '.tokage-live-acceptance').write_text('Tokage live CLI acceptance\n')
request = (BASE / 'docs/reliability-m3a.md').read_text().split('## m3a-001：', 1)[1]
goal = '''Implement m3a-001 in this real Tokage repository. SessionHistoryView must initially show latest output, follow new content when already at bottom, preserve reading position when scrolled up, and offer a keyboard accessible return-to-latest button. Reset intent on attempt switches; handle empty/loading/error/end states without stale scroll state. Observe rendered content height changes, including tool expansion. Avoid scroll anchoring fighting user intent. Add English/Chinese labels and meaningful controllable DOM regression tests (not just pure functions). You may add a small DOM test dev dependency and lockfile update if necessary. Keep changes within the declared scope. Do not edit .tokage, reliability docs, execution/auth/model routing, acceptance infrastructure, or weaken tests. Run npm ci --ignore-scripts --offline --no-audit --no-fund before frontend checks. The happy-dom package is prewarmed in the local npm cache if a DOM environment is needed. Run full frontend tests, typecheck, i18n checks and production build. Do not commit yourself: Tokage records delivery commits. Report under required TaskReport headings, contract deviations None if none.\n\nPreregistered acceptance:\n''' + request
if args.feedback_file:
    goal += '\n\nIndependent review findings from the previous attempt (original acceptance is unchanged):\n' + args.feedback_file.read_text()
if args.npm_cache_template:
    goal += '\nEnvironment repaired and independently verified: each worktree receives an independent writable npm cache at src-tauri/target/m3a-npm-cache. Use npm --cache ./src-tauri/target/m3a-npm-cache for installs, with --offline --ignore-scripts; happy-dom@21.0.3 and this project dependencies are seeded. For test commands, create src-tauri/target/m3a-tmp and set TMPDIR="$PWD/src-tauri/target/m3a-tmp". These paths are already ignored. Do not write HOME or relax sandbox policy. Actually render SessionHistoryView in a DOM regression; do not replace this with EventTarget-only tests or omit tests because the previous shared cache failed.'
commands = ['npm ci --ignore-scripts --offline --no-audit --no-fund', 'npm run typecheck', 'npm test', 'npm run check:i18n', 'npm run build']
plan = {'version': 1, 'isUserEdited': True, 'spec': {'raw': goal, 'goal': 'm3a-001 session transcript reading position', 'inScope': ['session transcript scrolling, bilingual labels, DOM regression tests'], 'outOfScope': ['auth, database, model routing, .tokage, acceptance infrastructure'], 'decisions': ['Codex 0.162.0 / gpt-6-luna only; independent review before merge']}, 'contracts': [], 'notes': 'One real requirement. No model fallback. Do not commit; Tokage handles task commits.', 'rawMarkdown': '', 'tasks': [{'id': 'm3a-001', 'domain': 'frontend', 'title': 'Session history follows latest output while preserving reading position', 'goal': goal, 'depends_on': [], 'contracts': [], 'write_scope': ['src/components/workspace/SessionHistoryView.tsx', 'src/components/workspace/SessionHistoryView.test.tsx', 'src/lib/sessionScroll*', 'src/hooks/*Scroll*', 'src/i18n/locales/en.ts', 'src/i18n/locales/zh.ts', 'package.json', 'package-lock.json'], 'deliverables': ['src/components/workspace/SessionHistoryView.tsx'], 'acceptance': {'commands': commands, 'confirmedCommands': ['npm ci --ignore-scripts --offline --no-audit --no-fund'], 'criteria': ['All preregistered m3a-001 behavior; meaningful DOM tests; separate real desktop streaming scroll verification before counting delivery']}, 'size': 'M', 'difficulty': 3, 'assigned_model': 'CODEX_CLI:gpt-6-luna', 'model_params': {}}]}
config = {'scenario': 'm3a', 'root': str(root), 'cwd': str(repo), 'provider': 'CODEX_CLI', 'model': 'gpt-6-luna', 'tier': None, 'versionPrefix': '0.162.0', 'cliPath': '/Users/iwpz/.nvm/versions/node/v22.16.0/bin/codex', 'request': goal, 'realPlan': plan}
if args.scroll_probe_repo:
    config['scrollProbe'] = True
    config['realPlan']['tasks'][0]['id'] = 'm3a-001-scroll-validation'
    config['realPlan']['tasks'][0]['goal'] = '''Read only: inspect src/components/workspace/SessionHistoryView.tsx, src/hooks/useSessionScroll.ts and src/lib/sessionScroll.ts. Do not write or commit any file. Explain the implementation and its edge cases in at least 80 short numbered paragraphs, with explicit references to actual functions. This is a genuine live-stream validation of the delivered feature: produce the full explanation as assistant text so a reader can inspect it while streaming. Discuss initial positioning, bottom following, reading above, returning to latest, attempt switches, loading, error, tool expansion and stream end. Use real read tools first. End with the required TaskReport headings. No edits are requested.'''
    config['realPlan']['tasks'][0]['acceptance']['commands'] = ['git diff --exit-code']
(root / 'config.json').write_text(json.dumps(config, ensure_ascii=False, indent=2))
(root / 'host.json').write_text(json.dumps({'base': git(BASE, 'rev-parse', 'HEAD'), 'originalRepository': str(BASE), 'candidateSource': str(args.source_repo) if args.source_repo else None, 'repositoryBase': git(repo, 'rev-parse', 'HEAD'), 'startedAt': time.time(), 'planReviewedBeforeDispatch': True}, indent=2))
print(root, flush=True)
with (root / 'desktop.log').open('w') as log:
    app = subprocess.Popen([str(BASE / 'src-tauri/target/debug/tokage')], env=dict(os.environ, TOKAGE_LIVE_DESKTOP_CONFIG=str(root / 'config.json')), stdout=log, stderr=subprocess.STDOUT)
    try:
        seeded = set()
        deadline = time.monotonic() + 2400
        while app.poll() is None:
            if args.npm_cache_template:
                for marker in (root / 'app-data/worktrees').glob('*/*/.git'):
                    cwd = marker.parent
                    if cwd in seeded: continue
                    target = cwd / 'src-tauri/target/m3a-npm-cache'
                    shutil.copytree(args.npm_cache_template, target, dirs_exist_ok=True)
                    (cwd / 'src-tauri/target/m3a-tmp').mkdir(parents=True, exist_ok=True)
                    seeded.add(cwd)
                    print(f'Seeded private writable dependency cache: {cwd}', flush=True)

            if time.monotonic() > deadline: raise TimeoutError(root)
            time.sleep(1)
        print(f'EXIT={app.returncode} evidence={root}', flush=True)
        if app.returncode: raise SystemExit(app.returncode)
    finally:
        if app.poll() is None: app.terminate(); app.wait(timeout=15)
