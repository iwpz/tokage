#!/usr/bin/env python3
"""Run rendered Tauri UI → real CLI DAG → gates → commits → merge in a fixture.

Build with VITE_TOKAGE_LIVE_ACCEPTANCE=1 and Cargo features
custom-protocol,live-acceptance before invoking this runner. No provider fallback.
"""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time

CHOICES = {
    "KIRO_CLI": ("claude-haiku-4.5", None),
    "AIDER": ("deepseek/deepseek-flash", None),
    "CLAUDE_CODE": ("haiku", None),
    "CURSOR_CLI": ("auto", None),
    "COPILOT_CLI": ("auto", "efficiency"),
    "ANTIGRAVITY_CLI": ("gemini-3.8-flash-low", None),
}

def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, check=True).stdout.strip()

def fixture(root):
    repo = root / "repo"
    repo.mkdir()
    files = {
        ".gitignore": ".tokage/\nnode_modules/\nempty-hooks/\n",
        "package.json": json.dumps({"name": "tokage-live-desktop-fixture", "private": True, "type": "module", "scripts": {"test:add": "node check-add.mjs", "test:integration": "node check-integration.mjs"}}),
        "calc.mjs": "export function add(a, b) { return a - b; }\n",
        "compose.mjs": "export function sumTwice(a, b) { return 0; }\n",
        "check-add.mjs": "import assert from 'node:assert/strict';\nimport {add} from './calc.mjs';\nassert.equal(add(2,3),5); assert.equal(add(-2,3),1); console.log('TOKAGE_ADD_GATE_OK');\n",
        "check-integration.mjs": "import assert from 'node:assert/strict';\nimport {add} from './calc.mjs';\nimport {sumTwice} from './compose.mjs';\nassert.equal(add(2,3),5); assert.equal(sumTwice(2,3),10); assert.equal(sumTwice(-2,3),2); console.log('TOKAGE_INTEGRATION_GATE_OK');\n",
    }
    for name, text in files.items(): (repo / name).write_text(text)
    (repo / "empty-hooks").mkdir()
    git(repo, "init", "-b", "main")
    git(repo, "config", "user.name", "Tokage acceptance")
    git(repo, "config", "user.email", "acceptance@tokage.invalid")
    git(repo, "config", "commit.gpgsign", "false")
    git(repo, "config", "core.hooksPath", "empty-hooks")
    git(repo, "add", ".")
    git(repo, "commit", "-m", "acceptance baseline")
    return repo, git(repo, "rev-parse", "HEAD"), files

def verify(root, repo, before, original, provider):
    result = json.loads((root / "result.json").read_text())
    assert result["status"] == "passed", f"{provider} failed: {result.get('error', 'unknown')[:500]}; evidence: {root}"
    snapshot = result["snapshot"]
    if provider == "KIRO_CLI":
        engine = json.loads((root / "config.json").read_text()).get("engine") or "v2"
        assert result.get("engine") == engine, result.get("engine")
        assert all(sid.startswith(f"kiro:{engine}:") for sid in snapshot["threadIds"].values()), snapshot["threadIds"]
    assert snapshot["taskStates"] == {"T1": "DONE", "T2": "DONE"}
    assert snapshot["delivery"]["phase"] == "merged" and snapshot["delivery"]["fullGatePassed"]
    assert snapshot["taskProviders"] == {"T1": provider, "T2": provider}
    assert snapshot["planDbId"] and len(snapshot["taskDbIds"]) == 2
    assert result["native"]["done"] >= 2 and not result["native"]["errors"]
    if provider == "COPILOT_CLI":
        assert result["native"]["premiumUsageEvents"] >= 2 and result["native"]["premiumRequests"] >= 0
    elif provider == "KIRO_CLI":
        assert result["native"]["creditUsageEvents"] >= 2 and result["native"]["credits"] > 0
    else:
        assert result["native"]["tokens"] > 0
    assert len(result["worktrees"]) == 2
    assert snapshot["taskStartedAt"]["T2"] >= snapshot["taskEndedAt"]["T1"]
    gates = snapshot["gateResults"]
    assert {g["taskId"] for g in gates} >= {"T1", "T2", "_delivery"}
    assert all(g["passed"] and g["exitCode"] == 0 for g in gates)
    assert any("TOKAGE_ADD_GATE_OK" in g["outputTail"] for g in gates)
    assert any("TOKAGE_INTEGRATION_GATE_OK" in g["outputTail"] for g in gates)
    assert git(repo, "rev-parse", "HEAD") != before
    changed = set(git(repo, "diff", "--name-only", before, "HEAD").splitlines())
    assert changed == {"calc.mjs", "compose.mjs"}, changed
    assert not git(repo, "status", "--porcelain")
    subjects = git(repo, "log", "--format=%s", before + "..HEAD")
    assert "Tokage task T1:" in subjects and "Tokage task T2:" in subjects
    for name in ["package.json", "check-add.mjs", "check-integration.mjs", ".gitignore"]:
        assert (repo / name).read_text() == original[name], name
    for script in ["test:add", "test:integration"]:
        subprocess.run(["npm", "run", script], cwd=repo, capture_output=True, text=True, check=True)
    with sqlite3.connect(root / "app-data" / "tokage.db") as db:
        usage = db.execute("SELECT u.provider_name, u.model_id, u.input_tokens, u.output_tokens, a.debug_output, u.api_calls, u.source FROM usage_events u LEFT JOIN attempts a ON a.id = u.attempt_id").fetchall()
        assert len(usage) >= 2, usage
        assert all(row[0] == provider and row[4] is None for row in usage), usage
        assert all(row[6] == ("STDOUT_REGEX" if provider == "AIDER" else "STRUCTURED") for row in usage), usage
        if provider == "COPILOT_CLI":
            assert all(row[5] and row[5] > 0 for row in usage), usage
        elif provider == "KIRO_CLI":
            credits = db.execute("SELECT credits, cost_usd FROM usage_events WHERE provider_name = ?", (provider,)).fetchall()
            assert all(c is not None and c >= 0 and usd is None for c, usd in credits), credits
            assert sum(c for c, _ in credits) > 0
        else:
            assert sum(row[2] + row[3] for row in usage) > 0
        attempts = db.execute("SELECT provider_name, status, workdir FROM attempts").fetchall()
        actual_attempts = [row for row in attempts if row[0] == provider]
        # The orchestration announcement creates an empty UNKNOWN/IDLE chat
        # placeholder; it is neither a CLI attempt nor a usage source.
        assert all(row[0] == provider or row == ("UNKNOWN", "IDLE", "") for row in attempts), attempts
        assert len(actual_attempts) >= 2 and all(row[1] == "DONE" and row[2] for row in actual_attempts), actual_attempts
    claude_usage = None
    if provider == "CLAUDE_CODE":
        assert len(result["native"]["models"]) >= 2 and all("haiku" in model.lower() for model in result["native"]["models"])
        with sqlite3.connect(root / "app-data" / "tokage.db") as db:
            totals = db.execute("SELECT SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens) FROM usage_events WHERE provider_name = ?", (provider,)).fetchone()
        assert totals[0] > 0 and totals[1] > 0 and totals[2] >= 0 and totals[3] >= 0, totals
        claude_usage = dict(zip(["inputTokens", "outputTokens", "cacheReadTokens", "cacheWriteTokens"], totals))
    proof = {"engine": json.loads((root / "config.json").read_text()).get("engine"), "provider": provider, "status": "passed", "root": str(root), "before": before, "after": git(repo, "rev-parse", "HEAD"), "changedFiles": sorted(changed), "actualUsageRows": len(usage), "gateCount": len(gates)}
    if claude_usage is not None: proof["usage"] = claude_usage
    (root / "verified.json").write_text(json.dumps(proof, indent=2))
    print(json.dumps(proof, ensure_ascii=False), flush=True)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("provider", choices=CHOICES)
    parser.add_argument("--binary", default="src-tauri/target/debug/tokage")
    parser.add_argument("--kiro-engine", choices=["v2", "v3"], default="v2")
    parser.add_argument("--timeout", type=int, default=900)
    args = parser.parse_args()
    root = Path(tempfile.mkdtemp(prefix="tokage-live-desktop-", dir="/tmp")).resolve()
    (root / ".tokage-live-acceptance").write_text("Tokage live CLI acceptance\n")
    repo, before, files = fixture(root)
    model, tier = CHOICES[args.provider]
    config = root / "config.json"
    config.write_text(json.dumps({"root": str(root), "cwd": str(repo), "provider": args.provider, "model": model, "tier": tier, "engine": args.kiro_engine if args.provider == "KIRO_CLI" else None}))
    env = dict(os.environ, TOKAGE_LIVE_DESKTOP_CONFIG=str(config))
    print(f"START {args.provider} model={model} tier={tier} engine={args.kiro_engine if args.provider == 'KIRO_CLI' else None} fixture={root}", flush=True)
    with (root / "desktop.log").open("w") as log:
        process = subprocess.Popen([str(Path(args.binary).resolve())], env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            end = time.monotonic() + args.timeout
            while process.poll() is None:
                if time.monotonic() >= end: raise TimeoutError(f"Desktop acceptance timed out; evidence: {root}")
                time.sleep(1)
            if not (root / "result.json").exists(): raise RuntimeError(f"Desktop exited {process.returncode} without an acceptance result; log: {root / 'desktop.log'}")
            verify(root, repo, before, files, args.provider)
        finally:
            if process.poll() is None:
                process.terminate()
                try: process.wait(timeout=10)
                except subprocess.TimeoutExpired: process.kill(); process.wait()

if __name__ == "__main__": main()
