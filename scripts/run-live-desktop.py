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
import platform
import hashlib
import live_natural_fixture as natural

CHOICES = {
    "CODEX_CLI": ("gpt-6-luna", None),
    "OPENCODE": ("deepseek/deepseek-flash", None),
    "REASONIX": ("deepseek/deepseek-flash", None),
    "KIRO_CLI": ("claude-haiku-4.5", None),
    "AIDER": ("deepseek/deepseek-flash", None),
    "CLAUDE_CODE": ("haiku", None),
    "CURSOR_CLI": ("auto", None),
    "COPILOT_CLI": ("auto", "efficiency"),
    "ANTIGRAVITY_CLI": ("gemini-3.8-flash-low", None),
}

def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, check=True).stdout.strip()

def fixture(root, scenario="fixed"):
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
    if scenario == "natural":
        for name in files:
            if name != ".gitignore": (repo/name).unlink()
        files = {".gitignore": files[".gitignore"], **natural.populate(repo)}
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
    if result.get("scenario") == "natural":
        repo = Path(result["deliveryCheckout"]).resolve()
        assert repo.is_relative_to(root), "Delivery checkout escaped fixture"
    config = json.loads((root/"config.json").read_text())
    if config.get("backgroundProbeSeconds"):
        events = [json.loads(line) for line in (root/"progress.jsonl").read_text().splitlines()]
        assert any(e.get("step") == "background-probe-passed" and config["backgroundProbeSeconds"] * 1000 <= e.get("backgroundElapsedMs",0) <= (config["backgroundProbeSeconds"] + 20) * 1000 for e in events)
    snapshot = result["snapshot"]
    if provider == "KIRO_CLI":
        engine = json.loads((root / "config.json").read_text()).get("engine") or "v2"
        assert result.get("engine") == engine, result.get("engine")
        assert all(sid.startswith(f"kiro:{engine}:") for sid in snapshot["threadIds"].values()), snapshot["threadIds"]
    is_natural = result.get("scenario") == "natural"
    ids = natural.verify(root, repo, original, result) if is_natural else {"T1", "T2"}
    assert snapshot["taskStates"] == {i: "DONE" for i in ids}
    assert snapshot["delivery"]["phase"] == "merged" and snapshot["delivery"]["fullGatePassed"]
    assert snapshot["taskProviders"] == {i: provider for i in ids}
    assert snapshot["planDbId"] and len(snapshot["taskDbIds"]) == len(ids)
    assert result["native"]["done"] >= 2 and not result["native"]["errors"]
    if provider == "COPILOT_CLI":
        assert result["native"]["premiumUsageEvents"] >= 2 and result["native"]["premiumRequests"] >= 0
    elif provider == "KIRO_CLI":
        assert result["native"]["creditUsageEvents"] >= 2 and result["native"]["credits"] > 0
    else:
        assert result["native"]["tokens"] > 0
    assert len(result["worktrees"]) == len(ids)
    if not is_natural: assert snapshot["taskStartedAt"]["T2"] >= snapshot["taskEndedAt"]["T1"]
    gates = snapshot["gateResults"]
    assert {g["taskId"] for g in gates} >= ids | {"_delivery"}
    assert all(g["passed"] and g["exitCode"] == 0 for g in gates)
    markers = ["TOKAGE_NATURAL_ALL_OK"] if is_natural else ["TOKAGE_ADD_GATE_OK", "TOKAGE_INTEGRATION_GATE_OK"]
    for marker in markers: assert any(marker in g["outputTail"] for g in gates)
    assert git(repo, "rev-parse", "HEAD") != before
    changed = set(git(repo, "diff", "--name-only", before, "HEAD").splitlines())
    assert changed == (set(natural.FILES) if is_natural else {"calc.mjs", "compose.mjs"}), changed
    assert not git(repo, "status", "--porcelain")
    subjects = git(repo, "log", "--format=%s", before + "..HEAD")
    assert all(f"Tokage task {i}:" in subjects for i in ids)
    for name in ([n for n in original if n not in natural.FILES] if is_natural else ["package.json", "check-add.mjs", "check-integration.mjs", ".gitignore"]):
        assert (repo / name).read_text() == original[name], name
    for script in (["test"] if is_natural else ["test:add", "test:integration"]):
        subprocess.run(["npm", "run", script], cwd=repo, capture_output=True, text=True, check=True)
    with sqlite3.connect(root / "app-data" / "tokage.db") as db:
        usage = db.execute("SELECT u.provider_name, u.model_id, u.input_tokens, u.output_tokens, a.debug_output, u.api_calls, u.source FROM usage_events u LEFT JOIN attempts a ON a.id = u.attempt_id").fetchall()
        assert len(usage) >= 2, usage
        assert all(row[0] == provider and row[4] is None for row in usage), usage
        if is_natural:
            assert all(row[1] == result["model"] for row in usage), "Model fallback detected in persisted usage"
        assert all(row[6] == ("STDOUT_REGEX" if provider == "AIDER" else "STRUCTURED") for row in usage), usage
        if provider == "COPILOT_CLI":
            assert all(row[5] and row[5] > 0 for row in usage), usage
        elif provider == "KIRO_CLI":
            credits = db.execute("SELECT credits, cost_usd FROM usage_events WHERE provider_name = ?", (provider,)).fetchall()
            assert all(c is not None and c >= 0 and usd is None for c, usd in credits), credits
            assert sum(c for c, _ in credits) > 0
        else:
            assert sum(row[2] + row[3] for row in usage) > 0
        attempts = db.execute("SELECT id, provider_name, status, workdir, task_id, role, model_id FROM attempts").fetchall()
        task_attempts = [row for row in attempts if row[4] is not None]
        assert len(task_attempts) >= len(ids)
        assert {row[4] for row in task_attempts} == set(snapshot["taskDbIds"].values())
        assert all(row[0].startswith(f"subagent-{result['sessionId']}-") and row[1] == provider and row[2] == "DONE" and row[3] and row[5] == "EXECUTOR" for row in task_attempts), task_attempts
        if json.loads((root / "config.json").read_text()).get("verifySessionStreaming"):
            for attempt in task_attempts:
                messages = db.execute("SELECT role, content, meta_json FROM chat_logs WHERE attempt_id=? ORDER BY seq", (attempt[0],)).fetchall()
                assert len(messages) == 2 and [m[0] for m in messages] == ["USER", "ASSISTANT"], "Missing task prompt/reply persistence"
                assert messages[0][1].strip() and messages[1][1].strip(), "Empty persisted task transcript"
                assert json.loads(messages[1][2])["status"] == "done", "Final transcript remained streaming"
            events = [json.loads(line) for line in (root / "progress.jsonl").read_text().splitlines()]
            streamed = [e for e in events if e.get("step") == "session-stream-verified"]
            assert len(streamed) >= (2 if is_natural else 1) and all(e["promptVisible"] and e["responseUpdatedWhileRunning"] and not e["transcriptPersistenceError"] for e in streamed)
        # Main-chat rows aggregate turns and return to IDLE with no workdir. They
        # are not task attempts. Validate the exact planner identity and its real
        # metered turn rather than accepting arbitrary unfinished executions.
        planner_id = f"{result['sessionId']}|{provider}|{result['model']}"
        unknown_id = f"{result['sessionId']}|UNKNOWN|"
        aggregates = [row for row in attempts if row[4] is None]
        allowed = {(unknown_id, "UNKNOWN", "IDLE", "", None, "EXECUTOR", "")}
        if is_natural:
            allowed.add((planner_id, provider, "IDLE", "", None, "PLANNER", result["model"]))
            assert any(row[0] == planner_id for row in aggregates)
            assert db.execute("SELECT COUNT(*) FROM usage_events WHERE attempt_id = ? AND source = 'STRUCTURED' AND input_tokens + output_tokens > 0", (planner_id,)).fetchone()[0] > 0
        assert all(row in allowed for row in aggregates), aggregates
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
    parser.add_argument("--verify-session-streaming", action="store_true")
    parser.add_argument("--background-probe-seconds", type=int, choices=range(0,601), default=0, metavar="0..600")
    parser.add_argument("--scenario", choices=["fixed", "natural"], default="fixed")
    parser.add_argument("--binary", default="src-tauri/target/debug/tokage")
    parser.add_argument("--cli-path")
    parser.add_argument("--version-prefix")
    parser.add_argument("--opencode-v1-auth", action="store_true", help="Use a temporary DeepSeek auth.json, never alter existing logins")
    parser.add_argument("--kiro-engine", choices=["v2", "v3"], default="v2")
    parser.add_argument("--timeout", type=int, default=900)
    args = parser.parse_args()
    if args.scenario == "natural" and args.provider not in {"CLAUDE_CODE", "OPENCODE", "CODEX_CLI"}: parser.error("Natural acceptance requires an explicitly registered fixed model target")
    root = Path(tempfile.mkdtemp(prefix="tokage-live-desktop-", dir="/tmp")).resolve()
    (root / ".tokage-live-acceptance").write_text("Tokage live CLI acceptance\n")
    repo, before, files = fixture(root, args.scenario)
    if args.scenario == "natural": natural.prepare(root, repo)
    (root/"environment-host.json").write_text(json.dumps({"platform":platform.platform(),"architecture":platform.machine(),"appCommit":git(Path.cwd(),"rev-parse","HEAD"),"dirty":bool(git(Path.cwd(),"status","--porcelain")),"sourceHashes":{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in [Path("src/lib/liveDesktopAcceptance.ts"),Path("src/lib/liveNaturalReview.ts"),Path("src-tauri/src/live_acceptance.rs"),Path("src-tauri/crates/tokage-planning/src/parser.rs"),Path("src-tauri/crates/tokage-planning/src/prompt.rs"),Path("src/components/planning/PlanReview.tsx"),Path("src-tauri/crates/tokage-native/src/context.rs"),Path("src-tauri/tauri.conf.json")]},"node":subprocess.run(["node","--version"],capture_output=True,text=True,check=True).stdout.strip(),"npm":subprocess.run(["npm","--version"],capture_output=True,text=True,check=True).stdout.strip()},indent=2))
    model, tier = CHOICES[args.provider]
    data_home = None
    try:
        if args.opencode_v1_auth:
            if args.provider != "OPENCODE": raise ValueError("V1 auth only applies to OpenCode")
            import shlex
            key = os.environ.get("DEEPSEEK_API_KEY")
            if not key:
                for line in (Path.home() / ".env").read_text().splitlines():
                    tokens = shlex.split(line, comments=True)
                    if tokens and tokens[0] == "export": tokens = tokens[1:]
                    if len(tokens) == 1 and tokens[0].startswith("DEEPSEEK_API_KEY="): key = tokens[0].split("=",1)[1]
            if not key or key.startswith("YOUR_"): raise RuntimeError("Configure DeepSeek locally; do not put a key on the command line")
            data_home = root / "opencode-v1-data"
            auth_dir = data_home / "opencode"; auth_dir.mkdir(parents=True, mode=0o700)
            auth_file = auth_dir / "auth.json"
            fd = os.open(auth_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd,"w") as f: json.dump({"deepseek":{"type":"api","key":key}},f)
            del key
        config = root / "config.json"
        config.write_text(json.dumps({"verifySessionStreaming":args.verify_session_streaming,"backgroundProbeSeconds":args.background_probe_seconds,"scenario":args.scenario,"request":natural.REQUEST if args.scenario == "natural" else None,"cliPath": str(Path(args.cli_path).resolve()) if args.cli_path else None, "dataHome": str(data_home) if data_home else None, "versionPrefix": args.version_prefix, "root": str(root), "cwd": str(repo), "provider": args.provider, "model": model, "tier": tier, "engine": args.kiro_engine if args.provider == "KIRO_CLI" else None}))
        env = dict(os.environ, TOKAGE_LIVE_DESKTOP_CONFIG=str(config))
        print(f"START {args.provider} model={model} tier={tier} engine={args.kiro_engine if args.provider == 'KIRO_CLI' else None} fixture={root}", flush=True)
        with (root / "desktop.log").open("w") as log:
            process = subprocess.Popen([str(Path(args.binary).resolve())], env=env, stdout=log, stderr=subprocess.STDOUT)
            try:
                end = time.monotonic() + args.timeout
                reviewed = False
                probe_started = None
                while process.poll() is None:
                    if args.background_probe_seconds and (root/"progress.jsonl").exists():
                        probe_events = []
                        for line in (root/"progress.jsonl").read_text().splitlines():
                            try: probe_events.append(json.loads(line))
                            except json.JSONDecodeError: continue
                        if probe_started is None and any(e.get("step") == "background-probe-start" for e in probe_events): probe_started = time.monotonic()
                        if probe_started is not None and not any(e.get("step") == "background-probe-passed" for e in probe_events) and time.monotonic() - probe_started > args.background_probe_seconds + 20:
                            raise TimeoutError(f"Background frontend did not advance within probe deadline; evidence: {root}")
                    if args.scenario == "natural" and not reviewed and (root/"progress.jsonl").exists():
                        lines = (root/"progress.jsonl").read_text().splitlines()
                        candidates = []
                        for line in lines:
                            try: event = json.loads(line)
                            except json.JSONDecodeError: continue # report writer may be mid-write
                            if event.get("step") == "independent-review-required": candidates.append(event)
                        if candidates:
                            event = candidates[-1]
                            try:
                                integration = Path(event["snapshot"]["integration"]["root"]).resolve()
                                assert integration.is_relative_to(root), "Integration escaped isolated fixture"
                                natural.verify(root, integration, files, event)
                                decision = {"passed":True,"head":git(integration,"rev-parse","HEAD")}
                            except Exception as error:
                                decision = {"passed":False,"error":str(error)}
                            (root/"independent-review.json").write_text(json.dumps(decision))
                            reviewed = True
                    if time.monotonic() >= end: raise TimeoutError(f"Desktop acceptance timed out; evidence: {root}")
                    time.sleep(1)
                if not (root / "result.json").exists(): raise RuntimeError(f"Desktop exited {process.returncode} without an acceptance result; log: {root / 'desktop.log'}")
                if process.returncode != 0:
                    failure = json.loads((root/"result.json").read_text())
                    raise RuntimeError(f"Desktop exited {process.returncode}: {str(failure.get('error', 'unknown'))[:800]}; evidence: {root}")
                verify(root, repo, before, files, args.provider)
            finally:
                if process.poll() is None:
                    process.terminate()
                    try: process.wait(timeout=10)
                    except subprocess.TimeoutExpired: process.kill(); process.wait()
    finally:
        if data_home: (data_home / "opencode/auth.json").unlink(missing_ok=True)

if __name__ == "__main__": main()
