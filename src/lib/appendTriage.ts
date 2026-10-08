/**
 * F16.e: route main-session composer messages while a pipeline is active.
 * Uses triage_main_session_message_cmd → task window / replan / previous direct / fresh direct.
 */

import {
  loadPlan,
  triageMainSessionMessage,
  type AppendMessageRoute,
  type Plan,
  type PlanDiff,
  type TaskSummaryForTriage,
} from "./ipc";
import type { ModelRef } from "./models";
import { canReplan, startReplanTurn } from "./replan";
import { useChat } from "../stores/chat";
import { useExecution } from "../stores/execution";
import { sessionCwd, useProjects } from "../stores/projects";

/** Last successful DIRECT turn in a session (for PREVIOUS_DIRECT_WINDOW). */
const lastDirectBySession = new Map<
  string,
  { attemptId: number; timestampMs: number; ref: ModelRef }
>();

let nextSyntheticAttemptId = 1;

export function recordLastDirectTurn(sessionId: string, ref: ModelRef): void {
  lastDirectBySession.set(sessionId, {
    attemptId: nextSyntheticAttemptId++,
    timestampMs: Date.now(),
    ref,
  });
}

export function getLastDirectPrefer(
  sessionId: string,
): { ref: ModelRef; mode: "direct" } | null {
  const last = lastDirectBySession.get(sessionId);
  if (!last) return null;
  return { ref: last.ref, mode: "direct" };
}

type ReplanReadyHandler = (payload: {
  sessionId: string;
  current: Plan;
  proposed: Plan;
  diff: PlanDiff;
}) => void;

let replanReadyHandler: ReplanReadyHandler | null = null;

/** ProjectWorkspace registers this to open PlanDiffView. */
export function setReplanReadyHandler(handler: ReplanReadyHandler | null): void {
  replanReadyHandler = handler;
}

/** Pending user follow-ups for a task execution window (consumed on continue / next dispatch). */
const pendingTaskFollowUps = new Map<string, string[]>();

function taskFollowUpKey(sessionId: string, taskId: string): string {
  return `${sessionId}::${taskId}`;
}

export function takePendingTaskFollowUp(
  sessionId: string,
  taskId: string,
): string | null {
  const key = taskFollowUpKey(sessionId, taskId);
  const list = pendingTaskFollowUps.get(key);
  if (!list?.length) return null;
  const next = list.shift()!;
  if (list.length === 0) pendingTaskFollowUps.delete(key);
  else pendingTaskFollowUps.set(key, list);
  return next;
}

export function peekPendingTaskFollowUp(
  sessionId: string,
  taskId: string,
): string | null {
  const list = pendingTaskFollowUps.get(taskFollowUpKey(sessionId, taskId));
  return list?.[0] ?? null;
}

function queueTaskFollowUp(sessionId: string, taskId: string, message: string): void {
  const key = taskFollowUpKey(sessionId, taskId);
  const list = pendingTaskFollowUps.get(key) ?? [];
  list.push(message);
  pendingTaskFollowUps.set(key, list);
}

export type AppendRouteResult =
  | { action: "direct"; preferPrevious: boolean; reason: string }
  | { action: "handled"; reason: string };

function activeTaskSummaries(sessionId: string): TaskSummaryForTriage[] {
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (!pipeline) return [];
  return pipeline.tasks
    .filter((t) => {
      const st = pipeline.taskStates[t.id] ?? "PENDING";
      return st !== "CANCELLED" && st !== "DONE";
    })
    .map((t) => ({
      taskId: t.id,
      title: t.title,
      writeScope: t.write_scope ?? [],
    }));
}

/**
 * When a pipeline exists for the session, triage the composer message.
 * Returns `direct` if the caller should run dispatchAsync (fresh or previous).
 * Returns `handled` when replan / task-window side effects already ran.
 */
export async function routePipelineAppend(
  sessionId: string,
  message: string,
): Promise<AppendRouteResult | null> {
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (!pipeline) return null;
  if (pipeline.status === "DONE") return null;

  const last = lastDirectBySession.get(sessionId);
  let route: AppendMessageRoute;
  try {
    route = await triageMainSessionMessage({
      message,
      activeTasks: activeTaskSummaries(sessionId),
      lastDirectAttemptId: last?.attemptId ?? null,
      lastDirectTimestampMs: last?.timestampMs ?? null,
      currentTimeMs: Date.now(),
    });
  } catch (err) {
    console.warn("[appendTriage] triage failed:", err);
    return null;
  }

  switch (route.type) {
    case "DIRECT_DISPATCH":
      return { action: "direct", preferPrevious: false, reason: route.reason };

    case "PREVIOUS_DIRECT_WINDOW":
      return { action: "direct", preferPrevious: true, reason: route.reason };

    case "TASK_EXECUTION_WINDOW": {
      const { taskId, taskTitle, reason } = route;
      queueTaskFollowUp(sessionId, taskId, message);
      useExecution
        .getState()
        .appendTaskLog(
          sessionId,
          taskId,
          `[主会话追加] ${message.slice(0, 200)}${message.length > 200 ? "…" : ""}`,
        );
      useChat.getState().postSystemMessage(
        sessionId,
        `[分流] 已排队到任务 ${taskId}（${taskTitle}）：${reason}`,
      );
      return { action: "handled", reason };
    }

    case "INCREMENTAL_REPLAN": {
      const gate = canReplan(sessionId);
      if (gate === "pipelineBusy") {
        useChat.getState().postSystemMessage(
          sessionId,
          `[分流] 检测到重规划意图（${route.reason}）。请先暂停流水线后再发送，或在计划页提出修改意见。`,
        );
        return { action: "handled", reason: route.reason };
      }
      const project = useProjects
        .getState()
        .projects.find((p) => p.sessions.some((s) => s.id === sessionId));
      if (!project) {
        return { action: "handled", reason: "no project" };
      }
      try {
        const cwd = sessionCwd(project, sessionId);
        const current = await loadPlan(cwd, sessionId);
        useChat.getState().postSystemMessage(
          sessionId,
          `[分流] 增量重规划：${route.reason}`,
        );
        const result = await startReplanTurn(sessionId, current, route.prompt);
        if ("error" in result) {
          useChat.getState().postSystemMessage(sessionId, `[重规划] ${result.error}`);
        } else {
          replanReadyHandler?.({
            sessionId,
            current,
            proposed: result.proposed,
            diff: result.diff,
          });
        }
      } catch (err) {
        useChat.getState().postSystemMessage(sessionId, `[重规划] 失败: ${err}`);
      }
      return { action: "handled", reason: route.reason };
    }

    default:
      return null;
  }
}
