/**
 * F16.c: incremental replan flow — assemble prompt → plan turn → diff → apply.
 */

import {
  applyPlanDiff,
  assembleReplanPrompt,
  computePlanDiff,
  generateAdaptationTasks,
  getTaskReports,
  parsePlanningOutput,
  persistPlan,
  recordPlanApproved,
  type Plan,
  type PlanDiff,
  type TaskExecutionReport,
  type TaskItem,
} from "./ipc";
import { planTurn } from "./turn";
import { useChat } from "../stores/chat";
import { useExecution } from "../stores/execution";
import { sessionCwd, useProjects } from "../stores/projects";

export type ReplanGate = "ok" | "noPlan" | "pipelineBusy" | "noModel" | "noSession";

/** Only allow replan when pipeline has not started or is paused. */
export function canReplan(sessionId: string): ReplanGate {
  const project = useProjects.getState().projects.find((p) =>
    p.sessions.some((s) => s.id === sessionId),
  );
  if (!project) return "noSession";
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (pipeline && pipeline.status === "RUNNING") return "pipelineBusy";
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
      const rows = await getTaskReports(t.id);
      const latest = rows[rows.length - 1];
      if (latest) {
        completedSummary = latest.summaryMd ?? "";
        fullReport = latest.summaryMd ?? null;
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
    return { error: "流水线运行中，请先暂停后再重规划。" };
  }
  if (gate === "noSession") return { error: "会话不存在。" };

  const project = useProjects.getState().projects.find((p) =>
    p.sessions.some((s) => s.id === sessionId),
  );
  if (!project) return { error: "项目不存在。" };
  const cwd = sessionCwd(project, sessionId);

  const reports = await taskReportsFor(currentPlan, sessionId);
  let prompt: string;
  try {
    prompt = await assembleReplanPrompt({
      currentPlan,
      changeDescription,
      taskReports: reports,
    });
  } catch (e) {
    return { error: `组装重规划提示失败: ${e}` };
  }

  const turn = planTurn(changeDescription, "plan");
  if (!turn) return { error: "无可用规划模型。" };

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
      return { error: last.error || "重规划轮次失败。" };
    }
    if (!last.text.trim()) continue;
    try {
      const proposed = await parsePlanningOutput({ markdown: last.text });
      const diff = await computePlanDiff(currentPlan, proposed);
      return { proposed, diff };
    } catch (e) {
      return { error: `解析重规划输出失败: ${e}` };
    }
  }
  return { error: "等待重规划超时。" };
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
  if (!project) return { error: "项目不存在。" };
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
    return { error: `应用差异失败: ${e}` };
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
    return { error: `保存计划失败: ${e}` };
  }

  const pipeline = useExecution.getState().pipelines[sessionId];
  if (pipeline && pipeline.status !== "DONE") {
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
    useExecution.setState((s) => {
      const p = s.pipelines[sessionId];
      if (!p) return s;
      return {
        pipelines: {
          ...s.pipelines,
          [sessionId]: {
            ...p,
            tasks: next.tasks,
            taskStates: states,
          },
        },
      };
    });
  }

  try {
    await recordPlanApproved({
      sessionId,
      plan: next,
      assignments: [],
      estCostUsd: 0,
    });
  } catch (e) {
    console.warn("[replan] record_plan_approved failed:", e);
  }

  return { plan: next, adaptationTasks };
}
