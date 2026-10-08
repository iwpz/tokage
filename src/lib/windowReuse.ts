import type { ActivePipelineSession } from "../stores/execution";
import type { TaskItem } from "./ipc";
import {
  evaluateWindowReuse,
  inTauri,
  type WindowReuseDecision,
  type WindowReuseParams,
} from "./ipc";

export interface WindowReuseCandidate {
  taskId: string;
  threadId: string;
  cwd: string;
  decision: WindowReuseDecision;
}

function estimateTokens(text: string): number {
  return Math.max(1, Math.ceil(text.length / 4));
}

function roleFromDomain(domain: string): string {
  if (domain === "testing") return "TESTER";
  if (domain === "review") return "REVIEWER";
  return "EXECUTOR";
}

/**
 * Pick a DONE peer window to resume for `task` when cost/cache rules allow.
 * Skips when not in Tauri, when worktree cwds differ, or when the thread is
 * already held by a running task.
 */
export async function findReusableWindow(options: {
  pipeline: ActivePipelineSession;
  task: TaskItem;
  model: string;
  prompt: string;
  /** Prefer same cwd; when set, only candidates with this cwd are considered. */
  preferredCwd?: string | null;
}): Promise<WindowReuseCandidate | null> {
  if (!inTauri) return null;

  const { pipeline, task, model, prompt, preferredCwd } = options;
  const roleB = roleFromDomain(task.domain);
  const now = Date.now();

  const occupiedThreads = new Set<string>();
  for (const [tid, state] of Object.entries(pipeline.taskStates)) {
    if (state === "RUNNING" || state === "REPAIRING" || state === "VERIFYING") {
      const th = pipeline.threadIds?.[tid];
      if (th) occupiedThreads.add(th);
    }
  }

  type Cand = {
    taskId: string;
    threadId: string;
    cwd: string;
    endedAt: number;
    peer: TaskItem;
    historyTokens: number;
  };
  const candidates: Cand[] = [];

  for (const peer of pipeline.tasks) {
    if (peer.id === task.id) continue;
    if (pipeline.taskStates[peer.id] !== "DONE") continue;
    const threadId = pipeline.threadIds?.[peer.id];
    if (!threadId || occupiedThreads.has(threadId)) continue;
    const cwd =
      pipeline.taskCwds?.[peer.id] ||
      pipeline.baseCwd ||
      pipeline.cwd ||
      ".";
    if (preferredCwd && cwd !== preferredCwd) continue;
    const endedAt = pipeline.taskEndedAt?.[peer.id] ?? 0;
    if (!endedAt) continue;
    const artifact = pipeline.upstreamArtifacts?.[peer.id];
    const historyTokens = artifact
      ? estimateTokens(artifact.report) + 8_000
      : 15_000;
    candidates.push({
      taskId: peer.id,
      threadId,
      cwd,
      endedAt,
      peer,
      historyTokens,
    });
  }

  candidates.sort((a, b) => b.endedAt - a.endedAt);

  for (const c of candidates) {
    const params: WindowReuseParams = {
      modelA: model,
      modelB: model,
      roleA: roleFromDomain(c.peer.domain),
      roleB,
      domainA: c.peer.domain,
      domainB: task.domain,
      writeScopeA: c.peer.write_scope ?? [],
      writeScopeB: task.write_scope ?? [],
      historyTokensA: c.historyTokens,
      briefTokensB: estimateTokens(prompt),
      baselineTokens: 25_000,
      expectedTurnsB: 2,
      apiCallsPerTurn: 4,
      growthPerTurn: 4_000,
      outputPerTurn: 1_000,
      elapsedSeconds: Math.max(0, (now - c.endedAt) / 1000),
      cacheTtlSeconds: 300,
      effectiveContext: 128_000,
      contextWindow: 200_000,
      cliAutoCompactRatio: 0.92,
      reworkPhi: 0.3,
      minSaving: 0.15,
      pIn: 3,
      pOut: 15,
      cacheReadMultiplier: 0.1,
      cacheWriteMultiplier: 1.25,
    };
    const decision = await evaluateWindowReuse(params);
    if (decision.canReuse) {
      return {
        taskId: c.taskId,
        threadId: c.threadId,
        cwd: c.cwd,
        decision,
      };
    }
  }
  return null;
}
