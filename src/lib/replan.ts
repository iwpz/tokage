/**
 * F16.c: incremental replan flow — assemble prompt → plan turn → diff → apply.
 */

import {
  applyPlanDiff,
  assembleReplanPrompt,
  computePlanDiff,
  generateAdaptationTasks,
  getTaskReports,
  loadPlan,
  parsePlanningOutput,
  persistPlan,
  recordPlanApproved,
  type Plan,
  type PlanDiff,
  type TaskExecutionReport,
  type TaskItem,
} from "./ipc";
import { resolvePlanningDomains } from "./planningDomains";
import { planTurn } from "./turn";
import { useChat } from "../stores/chat";
import { checkpointPipeline, useExecution } from "../stores/execution";
import { sessionCwd, useProjects } from "../stores/projects";
import i18n from "../i18n";

export type ReplanGate = "ok" | "noPlan" | "pipelineBusy" | "noModel" | "noSession";

/** Only allow replan when pipeline has not started or is paused. */
export function canReplan(sessionId: string): ReplanGate {
  const project = useProjects.getState().projects.find((p) =>
    p.sessions.some((s) => s.id === sessionId),
  );
  if (!project) return "noSession";
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (pipeline && (pipeline.status === "RUNNING" || Object.values(pipeline.taskStates).some((s) => ["RUNNING", "VERIFYING", "REPAIRING"].includes(s)))) return "pipelineBusy";
  return "ok";
}

async function taskReportsFor(plan: Plan, sessionId: string): Promise<TaskExecutionReport[]> {
  const pipeline = useExecution.getState().pipelines[sessionId];
  const reports: TaskExecutionReport[] = [];
  for (const t of plan.tasks) {
    const st = pipeline?.taskStates?.[t.id] ?? "PENDING";
    let completedSummary = "";
    let fullReport: string | null = null;
    let modifiedFiles: string[] = [];
    try {
      const rows = pipeline?.planDbId != null
        ? await getTaskReports(sessionId, pipeline.taskReportPlanIds?.[t.id] ?? pipeline.planDbId, t.id) : [];
      const latest = rows[rows.length - 1];
      if (latest) {
        completedSummary = latest.summaryMd ?? "";
        fullReport = latest.fullMd ?? null;
        modifiedFiles = latest.changedFilesList ?? [];
      }
    } catch {
      /* optional */
    }
    const status: TaskExecutionReport["status"] =
      st === "DONE"
        ? "DONE"
        : st === "CANCELLED"
          ? "CANCELLED"
          : st === "BLOCKED"
            ? "FAILED"
            : "PENDING";
    reports.push({
      taskId: t.id,
      status,
      completedSummary,
      fullReport,
      modifiedFiles,
    });
  }
  return reports;
}

/**
 * Kick off a replan plan-mode turn. Resolves with the proposed plan + diff after the turn ends.
 */
export async function startReplanTurn(
  sessionId: string,
  currentPlan: Plan,
  changeDescription: string,
): Promise<{ proposed: Plan; diff: PlanDiff } | { error: string }> {
  const gate = canReplan(sessionId);
  if (gate === "pipelineBusy") {
    return { error: i18n.t("replan.err.pipelineRunning") };
  }
  if (gate === "noSession") return { error: i18n.t("replan.err.noSession") };

  const project = useProjects.getState().projects.find((p) =>
    p.sessions.some((s) => s.id === sessionId),
  );
  if (!project) return { error: i18n.t("replan.err.noProject") };
  const cwd = sessionCwd(project, sessionId);

  const reports = await taskReportsFor(currentPlan, sessionId);
  let prompt: string;
  try {
    prompt = await assembleReplanPrompt({
      currentPlan,
      changeDescription,
      taskReports: reports,
      domains: resolvePlanningDomains(),
    });
  } catch (e) {
    return { error: i18n.t("replan.err.buildPrompt", { e }) };
  }

  const turn = planTurn(changeDescription, "plan", undefined, undefined, sessionId);
  if (!turn) return { error: i18n.t("replan.err.noModel") };

  const beforeCount = useChat.getState().conversations[sessionId]?.messages.length ?? 0;
  await useChat.getState().send({
    sessionId,
    cwd,
    text: prompt,
    ...turn,
    framePlanningPrompt: false,
  });

  // Wait up to 10 minutes for the plan turn to finish.
  const deadline = Date.now() + 10 * 60_000;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 400));
    const msgs = useChat.getState().conversations[sessionId]?.messages ?? [];
    if (msgs.length <= beforeCount) continue;
    const last = [...msgs].reverse().find((m) => m.role === "assistant");
    if (!last || last.status === "streaming" || last.status === "waiting_approval") continue;
    if (last.status === "error" || last.status === "cancelled") {
      return { error: last.error || i18n.t("replan.err.turnFailed") };
    }
    if (!last.text.trim()) continue;
    try {
      const proposed = await parsePlanningOutput({ markdown: last.text });
      const diff = await computePlanDiff(currentPlan, proposed);
      return { proposed, diff };
    } catch (e) {
      return { error: i18n.t("replan.err.parse", { e }) };
    }
  }
  return { error: i18n.t("replan.err.timeout") };
}

/** Loads the saved plan and asks the planner for a revision; the caller shows the diff. */
export async function replanSession(
  sessionId: string,
  changeDescription: string,
): Promise<{ current: Plan; proposed: Plan; diff: PlanDiff } | { error: string }> {
  const project = useProjects.getState().projects.find((p) =>
    p.sessions.some((s) => s.id === sessionId),
  );
  if (!project) return { error: i18n.t("replan.err.noProject") };
  try {
    const current = await loadPlan(sessionCwd(project, sessionId), sessionId);
    const result = await startReplanTurn(sessionId, current, changeDescription);
    return "error" in result ? result : { current, ...result };
  } catch (e) {
    return { error: String(e) };
  }
}

/** Apply confirmed replan: version bump, adaptation tasks, persist, optional pipeline update. */
export async function confirmReplan(args: {
  sessionId: string;
  currentPlan: Plan;
  proposed: Plan;
  diff: PlanDiff;
}): Promise<{ plan: Plan; adaptationTasks: TaskItem[] } | { error: string }> {
  const { sessionId, currentPlan, proposed, diff } = args;
  const project = useProjects.getState().projects.find((p) =>
    p.sessions.some((s) => s.id === sessionId),
  );
  if (!project) return { error: i18n.t("replan.err.noProject") };
  if (canReplan(sessionId) === "pipelineBusy") return { error: i18n.t("replan.err.pipelineRunning") };
  const cwd = sessionCwd(project, sessionId);

  let next: Plan;
  try {
    next = await applyPlanDiff(
      currentPlan,
      diff,
      proposed,
      currentPlan.version?.toString() ?? null,
    );
  } catch (e) {
    return { error: i18n.t("replan.err.applyDiff", { e }) };
  }

  const changedContractIds = diff.contracts
    .filter((c) => c.action === "modified" || c.action === "added")
    .map((c) => c.id);

  const reports = await taskReportsFor(currentPlan, sessionId);
  let adaptationTasks: TaskItem[] = [];
  try {
    adaptationTasks = await generateAdaptationTasks({
      currentPlan: next,
      changedContractIds,
      taskReports: reports,
    });
  } catch (e) {
    console.warn("[replan] generateAdaptationTasks failed:", e);
  }

  if (adaptationTasks.length > 0) {
    const existingIds = new Set(next.tasks.map((t) => t.id));
    next = {
      ...next,
      tasks: [...next.tasks, ...adaptationTasks.filter((t) => !existingIds.has(t.id))],
    };
  }

  try {
    await persistPlan(cwd, sessionId, next);
  } catch (e) {
    return { error: i18n.t("replan.err.save", { e }) };
  }

  const pipeline = useExecution.getState().pipelines[sessionId];
  if (pipeline) {
    const removed = new Set(
      diff.tasks.filter((t) => t.action === "removed").map((t) => t.id),
    );
    const states = { ...pipeline.taskStates };
    for (const id of removed) {
      states[id] = "CANCELLED";
    }
    for (const t of next.tasks) {
      if (!states[t.id]) states[t.id] = "PENDING";
    }
    // A task whose definition changed cannot resume its old CLI session: the session knows
    // the old goal. Dropping the thread id makes "continue" dispatch it fresh from the new brief.
    const redefined = new Set(
      diff.tasks
        .filter((t) => t.action !== "unchanged")
        .map((t) => t.id),
    );
    for (const id of redefined) {
      if (!removed.has(id)) states[id] = "PENDING";
    }
    const keep = <T>(values: Record<string, T> | undefined): Record<string, T> =>
      Object.fromEntries(Object.entries(values ?? {}).filter(([id]) => !redefined.has(id)));
    const threadIds = Object.fromEntries(
      Object.entries(pipeline.threadIds ?? {}).filter(([id]) => !redefined.has(id)),
    );
    useExecution.setState((s) => {
      const p = s.pipelines[sessionId];
      if (!p) return s;
      return {
        pipelines: {
          ...s.pipelines,
          [sessionId]: {
            ...p,
            tasks: next.tasks,
            contracts: next.contracts,
            taskReportPlanIds: Object.fromEntries(next.tasks
              .filter((t) => states[t.id] === "DONE" && !redefined.has(t.id) && p.planDbId != null)
              .map((t) => [t.id, p.taskReportPlanIds?.[t.id] ?? p.planDbId!])),
            taskStates: states,
            status: "PAUSED",
            threadIds,
            taskTurnIds: keep(p.taskTurnIds),
            taskLogs: keep(p.taskLogs),
            taskStartedAt: keep(p.taskStartedAt),
            taskEndedAt: keep(p.taskEndedAt),
            taskFailureEvidence: keep(p.taskFailureEvidence),
            taskCwds: keep(p.taskCwds),
            taskProviders: keep(p.taskProviders),
            taskWindowConfigs: keep(p.taskWindowConfigs),
            repairSteps: keep(p.repairSteps),
            upstreamArtifacts: Object.fromEntries(Object.entries(p.upstreamArtifacts ?? {}).filter(([id]) => !redefined.has(id))),
            gateResults: [],
            taskWorktrees: Object.fromEntries(Object.entries(p.taskWorktrees ?? {}).filter(([id]) => !redefined.has(id))),
            taskApprovals: {},
            taskAllowed: {},
            taskCostUsd: {},
            delivery: null,
            planDbId: null,
            taskDbIds: {},
          },
        },
      };
    });
  }

  try {
    const recorded = await recordPlanApproved({
      sessionId,
      plan: next,
      assignments: [],
      estCostUsd: 0,
    });
    useExecution.getState().setPlanDbIds(sessionId, recorded.planId, recorded.taskIds);
  } catch (e) {
    console.warn("[replan] record_plan_approved failed:", e);
    if (pipeline) await checkpointPipeline(sessionId);
    return { error: String(e) };
  }
  if (pipeline) await checkpointPipeline(sessionId);

  return { plan: next, adaptationTasks };
}
