#!/usr/bin/env python3
"""Repeatable local checks or explicitly opted-in paid desktop acceptance.

Each invocation creates a fresh evidence directory, retains every command's exit
code and reports passed / failed / skipped separately. No CI or live success is
inferred from local fixtures. A skipped live target fails acceptance.
"""
from __future__ import annotations
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def local_checks() -> list[tuple[str, list[str]]]:
    return [
        ('harness', [sys.executable, '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_reliability.py']),
        ('m2g-harness', [sys.executable, '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_m2g_desktop.py']),
        ('test-isolation', [sys.executable, 'scripts/check-test-isolation.py']),
        ('isolation-harness', [sys.executable, '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_test_isolation.py']),
        ('typecheck', ['npm', 'run', 'typecheck']),
        ('frontend', ['npm', 'test', '--', '--maxWorkers=2']),
        # Skip readiness-only host probing; all model-dependent suites stay ignored.
        ('rust', ['cargo', 'test', '--manifest-path', 'src-tauri/Cargo.toml', '--workspace', '--locked', '--jobs', '2', '--', '--skip', 'chat::live::smoke_detect_skip_if_absent', '--test-threads=2']),
        ('build', ['npm', 'run', 'build']),
        ('i18n', ['npm', 'run', 'check:i18n']),
        ('ipc', [sys.executable, 'scripts/check-ipc-wiring.py']),
        ('cli-baselines', [sys.executable, 'scripts/check-cli-baselines.py']),
        ('m2c-artifacts', [sys.executable, 'scripts/verify-m2c-git-evidence.py']),
        ('m2e-artifacts', [sys.executable, 'scripts/verify-m2e-recovery-evidence.py']),
        ('m2g-artifacts', [sys.executable, 'scripts/verify-m2g-evidence.py']),
    ]


def run_check(name: str, command: list[str], output: Path, *, live: bool, cwd: Path = ROOT) -> dict:
    log = output / f'{name}.log'
    env = dict(os.environ)
    if not live:
        # Never inherit a developer's opt-in flag into the default regression run.
        env.pop('TOKAGE_LIVE_SMOKE', None)
    started = datetime.now(timezone.utc).isoformat()
    print(f'RUN {name}: {command}', flush=True)
    try:
        with log.open('w') as stream:
            result = subprocess.run(command, cwd=cwd, env=env, stdout=stream, stderr=subprocess.STDOUT)
        code = result.returncode
    except OSError as error:
        log.write_text(str(error) + '\n')
        code = 127
    text = log.read_text(errors='replace')
    skips = len(re.findall(r'^\s*(?:skip\b|SKIP\b)', text, re.M))
    status = 'failed' if code != 0 else ('skipped' if live and skips else 'passed')
    record = dict(name=name, command=command, exitCode=code, status=status, skipLines=skips, startedAt=started, log=log.name)
    if name == 'rust':
        matches = re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;.*? (\d+) filtered out;', text)
        record['tests'] = {key: sum(int(row[i]) for row in matches) for i, key in enumerate(['passed', 'failed', 'ignored', 'filtered'])}
        if not matches or record['tests']['passed'] == 0 or record['tests']['failed'] > 0:
            record['status'] = 'failed'
    print(f"{record['status'].upper()} {name}: exit={code}, skipLines={skips}; {log}", flush=True)
    return record


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=['local', 'live'], default='local')
    parser.add_argument('--allow-paid', action='store_true', help='Explicit permission to spend model quota in live mode')
    parser.add_argument('--provider', action='append', choices=['CLAUDE_CODE', 'OPENCODE'])
    parser.add_argument('--output', type=Path, help='New directory only; default is outside the repository')
    args = parser.parse_args(argv)
    if args.mode == 'live' and not args.allow_paid:
        parser.error('live mode spends model quota: --allow-paid is required')
    if args.mode == 'local' and (args.allow_paid or args.provider):
        parser.error('--allow-paid / --provider require --mode live')
    output = args.output.resolve() if args.output else Path(tempfile.mkdtemp(prefix='tokage-reliability-'))
    if args.output:
        output.mkdir(parents=True, exist_ok=False)
    checks = local_checks() if args.mode == 'local' else [(p, ['bash', 'scripts/run-m2b-desktop.sh', p]) for p in dict.fromkeys(args.provider or ['CLAUDE_CODE', 'OPENCODE'])]
    records = []
    report = dict(mode=args.mode, paid=args.mode == 'live', checks=records)
    for name, command in checks:
        records.append(run_check(name, command, output, live=args.mode == 'live'))
        report['counts'] = {status: sum(r['status'] == status for r in records) for status in ['passed', 'failed', 'skipped']}
        (output / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"SUMMARY {report['counts']}; evidence={output}", flush=True)
    return 0 if all(r['status'] == 'passed' for r in records) else 1


if __name__ == '__main__':
    raise SystemExit(main())
