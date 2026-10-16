import { gitCaptureRollback, gitScopeSnapshot, validateWriteScope, type ScopeSnapshot } from "./ipc";
import { checkpointPipeline, useExecution } from "../stores/execution";

// A shared checkout cannot attribute concurrent writes to individual agents.
const owners = new Map<string, string>();
const pins = new Map<string, number>();
const deferredReleases = new Set<string>();
export function pinScopeOwner(turnId: string): () => void {
  pins.set(turnId, (pins.get(turnId) ?? 0) + 1);
  return () => {
    const count = (pins.get(turnId) ?? 1) - 1;
    if (count) pins.set(turnId, count);
    else { pins.delete(turnId); if (deferredReleases.delete(turnId)) releaseScopeOwner(turnId); }
  };
}
export function releaseScopeOwner(turnId: string): void {
  if (pins.has(turnId)) { deferredReleases.add(turnId); return; }
  for (const [root, owner] of owners) if (owner === turnId) owners.delete(root);
}
export function resetScopeOwners(): void { owners.clear(); pins.clear(); deferredReleases.clear(); }

function own(snapshot: ScopeSnapshot, turnId: string): void {
  const existing = owners.get(snapshot.root);
  if (existing && existing !== turnId) throw new Error("Another task owns this shared checkout; write attribution is unavailable");
  owners.set(snapshot.root, turnId);
}

/** Capture once before the first CLI write; resumes keep the original baseline. */
export async function prepareScopeBaseline(sessionId: string, taskId: string, turnId: string, workdir: string): Promise<void> {
  const current = await gitScopeSnapshot(workdir);
  own(current, turnId);
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (!pipeline) throw new Error("Missing pipeline for scope baseline");
  let previous = pipeline.scopeBaselines?.[taskId];
  const wt = pipeline.taskWorktrees?.[taskId];
  if (previous && previous.root !== current.root && wt?.root === current.root && wt.cwd === current.workdir && wt.baseCommit === current.head && !pipeline.threadIds?.[taskId]) previous = undefined;
  if (!previous && wt && !current.clean) throw new Error("New task worktree is not clean; existing changes cannot be included safely");
  if (previous && (previous.root !== current.root || previous.workdir !== current.workdir || previous.head !== current.head)) {
    throw new Error("Execution baseline no longer matches the checkout");
  }
  if (!previous && pipeline.threadIds?.[taskId]) throw new Error("Cannot resume without the original scope baseline");
  await gitCaptureRollback(workdir, turnId, current);
  useExecution.setState((state) => {
    const p = state.pipelines[sessionId];
    if (!p || p.taskStates[taskId] !== "RUNNING" || p.taskTurnIds?.[taskId] !== turnId) throw new Error("Task stopped while capturing scope baseline");
    return { pipelines: { ...state.pipelines, [sessionId]: { ...p,
      scopeBaselines: { ...p.scopeBaselines, [taskId]: { ...(previous ?? current), executionMode: previous?.executionMode ?? (wt ? "worktree" : "inplace") } },
    } } };
  });
  await checkpointPipeline(sessionId);
  const live = useExecution.getState().pipelines[sessionId];
  if (live?.taskStates[taskId] !== "RUNNING" || live.taskTurnIds?.[taskId] !== turnId) throw new Error("Task stopped before CLI launch");
}

export class ScopeViolation extends Error {
  constructor(public changedFiles: string[], public outOfScopeFiles: string[]) {
    super(`Out-of-scope writes: ${outOfScopeFiles.join(", ")}`);
  }
}

export interface ScopeEvidence { snapshot: ScopeSnapshot; changedFiles: string[]; checkedAt: number }

/** Always read disk, even with zero CLI file events. Throws on unavailable or ambiguous evidence. */
export async function checkTaskScope(sessionId: string, taskId: string, turnId: string, workdir: string,
  writeScope: string[], cliFiles: string[] = []): Promise<ScopeEvidence> {
  const pipeline = useExecution.getState().pipelines[sessionId];
  const baseline = pipeline?.scopeBaselines?.[taskId];
  if (!baseline) throw new Error("Missing execution scope baseline; scope cannot be verified");
  own(baseline, turnId);
  for (const p of Object.values(useExecution.getState().pipelines)) {
    for (const [id, other] of Object.entries(p.scopeBaselines ?? {})) {
      if (p.sessionId === sessionId && id === taskId) continue;
      if (other.root === baseline.root && ["RUNNING", "VERIFYING", "REPAIRING"].includes(p.taskStates[id])) {
        throw new Error("Concurrent tasks share this checkout; changes cannot be attributed safely");
      }
    }
  }
  const current = await gitScopeSnapshot(workdir);
  if (current.root !== baseline.root || current.workdir !== baseline.workdir || current.head !== baseline.head) {
    throw new Error("Repository, directory or HEAD changed since execution baseline");
  }
  const changed = new Set<string>();
  const prefix = current.prefix ? `${current.prefix}/` : "";
  for (const path of new Set([...Object.keys(baseline.files), ...Object.keys(current.files)])) {
    const before = baseline.files[path], after = current.files[path];
    if (before?.disk !== after?.disk || before?.index !== after?.index) {
      changed.add(path.startsWith(prefix) ? path.slice(prefix.length) : `../${path}`);
    }
  }
  const actualPaths = [...changed].sort();
  // CLI evidence may reveal transient/outside writes; it can add suspicions, never remove disk changes.
  for (const path of cliFiles) {
    const normalized = path.replace(/\\/g, "/");
    const base = current.workdir.replace(/\\/g, "/").replace(/\/$/, "") + "/";
    changed.add(normalized.startsWith(base) ? normalized.slice(base.length) : normalized);
  }
  const paths = [...changed].sort();
  if (paths.some((p) => p.startsWith("/") || /^[A-Za-z]:/.test(p) || p.split(/[\\/]/).includes(".."))) {
    throw new Error(`Changes outside the task directory: ${paths.join(", ")}`);
  }
  const result = await validateWriteScope(writeScope, paths);
  const allowed = new Set(result.allowedFiles);
  if (!result.passed || result.outOfScopeFiles.length || paths.some((p) => !allowed.has(p))) {
    const outside = paths.filter((p) => !allowed.has(p));
    throw new ScopeViolation(paths, [...new Set([...outside, ...result.outOfScopeFiles])]);
  }
  const live = useExecution.getState().pipelines[sessionId];
  if (!live || !["RUNNING", "VERIFYING", "REPAIRING"].includes(live.taskStates[taskId])) throw new Error("Task stopped during scope verification");
  const evidence = { snapshot: current, changedFiles: actualPaths, checkedAt: Date.now() };
  useExecution.setState((state) => {
    const p = state.pipelines[sessionId];
    if (!p) throw new Error("Pipeline disappeared during scope verification");
    return { pipelines: { ...state.pipelines, [sessionId]: { ...p, scopeChecks: { ...p.scopeChecks, [taskId]: evidence } } } };
  });
  await checkpointPipeline(sessionId);
  return evidence;
}

/** Keep pipeline dispatch out of a checkout while a manual rollback is being reviewed/applied. */
export async function withRollbackCheckout<T>(workdir: string, action: () => Promise<T>): Promise<T> {
  const snapshot = await gitScopeSnapshot(workdir);
  const token = `rollback:${crypto.randomUUID()}`;
  for (const pipeline of Object.values(useExecution.getState().pipelines)) {
    for (const [id, baseline] of Object.entries(pipeline.scopeBaselines ?? {})) {
      if (baseline.root === snapshot.root && ["RUNNING", "VERIFYING", "REPAIRING"].includes(pipeline.taskStates[id])) {
        throw new Error("A task is still writing this checkout; stop it before restoring files");
      }
    }
  }
  own(snapshot, token);
  try { return await action(); } finally { releaseScopeOwner(token); }
}
