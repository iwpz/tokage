import { create } from "zustand";
import {
  getTaskReports,
  loadPlan,
  loadPipelineState,
  savePipelineState,
  type FailureEvidence,
  type PipelineGateResult,
  type PipelineState,
  type PipelineStatus,
  type PlanContract,
  type DeliverySummary,
  type IntegrationInfo,
  type TaskAssignment,
  type TaskItem,
  type TaskState,
  type TaskWorktreeInfo,
  type UpstreamArtifact,
} from "../lib/ipc";
import { canTransition } from "../lib/taskState";

/** F11.f plan delivery UI state (in-memory). */
export type DeliveryPhase =
  | "idle"
  | "running_gates"
  | "ready"
  | "merged"
  | "exported"
  | "kept"
  | "failed";

export interface PlanDeliveryState {
  phase: DeliveryPhase;
  summary?: DeliverySummary | null;
  fullGatePassed?: boolean;
  patchPath?: string | null;
  error?: string | null;
}

/** Failure evidence plus the directory the task ran in (used by 「还原越界文件」). */
export interface TaskFailureRecord extends FailureEvidence {
  workdir: string;
}

/** Pending or resolved CLI approval for a pipeline task (F6). */
export interface TaskApproval {
  id: string;
  turnId: string;
  toolId: string | null;
  command: string;
  level: string;
  reason: string;
  recoverable: boolean | null;
  resolved: boolean;
  decision?: "allow_once" | "allow_session" | "allow_project" | "deny";
  /** Epoch ms when the approval was first seen (unified queue ordering). */
  createdAtMs?: number;
}

/** R9 window fingerprint stored at first turn for resume continuity checks. */
export interface TaskWindowConfig {
  model: string;
  role: string;
  startupArgs: string[];
}

export interface ActivePipelineSession {
  sessionId: string;
  status: PipelineStatus;
  tasks: TaskItem[];
  contracts?: PlanContract[];
  /** Snapshot of matching-engine assignments at plan approval (F1.f / S6). */
  assignments?: TaskAssignment[];
  upstreamArtifacts?: Record<string, UpstreamArtifact>;
  taskStates: Record<string, TaskState>;
  taskLogs: Record<string, string[]>;
  taskTurnIds?: Record<string, string>;
  taskStartedAt?: Record<string, number>;
  taskEndedAt?: Record<string, number>;
  gateResults?: PipelineGateResult[];
  /** In-memory failure evidence per task (R6 out-of-scope files, F5.i). */
  taskFailureEvidence?: Record<string, TaskFailureRecord>;
  /** taskId → latest CLI thread id from `thread` events (F6.a; persist in F7). */
  threadIds?: Record<string, string>;
  /** taskId → approval items for the board (F6). */
  taskApprovals?: Record<string, TaskApproval[]>;
  /** taskId → approved commands for sessionAllowed resume (F6). */
  taskAllowed?: Record<string, string[]>;
  /** F7 / F12: plans.id when known. */
  planDbId?: number | null;
  /** F12: task code → tasks.id (integer PK). */
  taskDbIds?: Record<string, number>;
  /** F7: session working directory (same as cwd; explicit for v2 payload). */
  baseCwd?: string;
  /** F7: taskId → actual task working directory. */
  taskCwds?: Record<string, string>;
  /** F7: taskId → provider used for the last turn. */
  taskProviders?: Record<string, string>;
  /** R9: taskId → window fingerprint at first start (in-memory; re-set on resume). */
  taskWindowConfigs?: Record<string, TaskWindowConfig>;
  /** F7 / F11: task worktree metadata. */
  taskWorktrees?: Record<string, TaskWorktreeInfo>;
  /** F11: session integration branch + worktree. */
  integration?: IntegrationInfo | null;
  /** F11: attempt counters per task id for worktree branch naming. */
  taskAttemptCounts?: Record<string, number>;
  /** F7 / F13: repair step labels. */
  repairSteps?: Record<string, string>;
  /** F7 / F13: per-task cost. */
  taskCostUsd?: Record<string, number>;
  /** F7 / F9: tasks with unconfirmed contract deviations. */
  deviationSources?: string[];
  /**
   * F9: in-memory wait reasons from last pick (not persisted).
   * taskId → DispatchIneligibleReason (opaque JSON-compatible object).
   */
  taskWaitReasons?: Record<string, unknown>;
  /**
   * F7: v1 file could not be backfilled from the current plan — discard only.
   */
  unrecoverable?: boolean;
  /** F11.f: delivery summary + three-way action state (not persisted). */
  delivery?: PlanDeliveryState | null;
  startedAt: number;
  cwd?: string;
}

interface ExecutionState {
  // sessionId -> ActivePipelineSession
  pipelines: Record<string, ActivePipelineSession>;
  startPlanExecution: (
    sessionId: string,
    tasks: TaskItem[],
    cwd?: string,
    contracts?: PlanContract[],
    assignments?: TaskAssignment[],
  ) => void;
  updateTaskState: (
    sessionId: string,
    taskId: string,
    state: TaskState,
    log?: string,
    turnId?: string,
  ) => void;
  /**
   * Atomically claim READY → RUNNING for dispatch. Returns false if already claimed.
   */
  claimTaskRunning: (
    sessionId: string,
    taskId: string,
    turnId: string,
    log?: string,
  ) => boolean;
  appendTaskLog: (sessionId: string, taskId: string, log: string) => void;
  recordGateResult: (sessionId: string, result: PipelineGateResult) => void;
  recordTaskFailureEvidence: (
    sessionId: string,
    taskId: string,
    evidence: TaskFailureRecord | null,
  ) => void;
  setTaskThreadId: (sessionId: string, taskId: string, threadId: string) => void;
  upsertTaskApproval: (sessionId: string, taskId: string, approval: TaskApproval) => void;
  resolveTaskApproval: (
    sessionId: string,
    taskId: string,
    approvalId: string,
    decision: NonNullable<TaskApproval["decision"]>,
  ) => void;
  addTaskAllowedCommand: (sessionId: string, taskId: string, command: string) => void;
  setTaskCwd: (sessionId: string, taskId: string, cwd: string) => void;
  setTaskProvider: (sessionId: string, taskId: string, provider: string) => void;
  setTaskWindowConfig: (
    sessionId: string,
    taskId: string,
    config: TaskWindowConfig,
  ) => void;
  setTaskWorktree: (
    sessionId: string,
    taskId: string,
    worktree: TaskWorktreeInfo | null,
  ) => void;
  setIntegration: (sessionId: string, integration: IntegrationInfo | null) => void;
  setDelivery: (sessionId: string, delivery: PlanDeliveryState | null) => void;
  setPlanDbIds: (
    sessionId: string,
    planDbId: number,
    taskDbIds: Record<string, number>,
  ) => void;
  nextTaskAttempt: (sessionId: string, taskId: string) => number;
  discardPipeline: (sessionId: string) => void;
  recordTaskArtifact: (sessionId: string, taskId: string, artifact: UpstreamArtifact) => void;
  pauseExecution: (sessionId: string) => void;
  resumeExecution: (sessionId: string) => void;
  updateTaskAssignedModel: (sessionId: string, taskId: string, modelSpec: string) => void;
  /** F13.c: persist repair ladder step per task. */
  setRepairStep: (sessionId: string, taskId: string, step: string) => void;
  updatePipelineTasks: (sessionId: string, tasks: TaskItem[]) => void;
  setTaskWaitReasons: (
    sessionId: string,
    reasons: Record<string, unknown>,
  ) => void;
  addDeviationSource: (sessionId: string, taskId: string) => void;
  confirmDeviationSource: (sessionId: string, taskId: string) => void;
  loadPipelineFromDisk: (cwd: string, sessionId: string) => Promise<ActivePipelineSession | null>;
  getPipeline: (sessionId: string) => ActivePipelineSession | undefined;
}

const persistTimers: Record<string, ReturnType<typeof setTimeout>> = {};

/** Placeholder only used when marking unrecoverable v1 pipelines. */
function placeholderTask(t: { id: string; title: string; assignedModel?: string | null }): TaskItem {
  return {
    id: t.id,
    title: t.title,
    domain: "backend",
    goal: "",
    depends_on: [],
    contracts: [],
    write_scope: [],
    deliverables: [],
    acceptance: { commands: [], criteria: [] },
    size: "M",
    difficulty: 2,
    assigned_model: t.assignedModel ?? undefined,
  };
}

function persistSessionImmediate(session: ActivePipelineSession) {
  if (!session.cwd) return;
  if (persistTimers[session.sessionId]) {
    clearTimeout(persistTimers[session.sessionId]);
    delete persistTimers[session.sessionId];
  }
  const assignmentsMap: Record<string, TaskAssignment> = {};
  if (session.assignments) {
    for (const a of session.assignments) {
      assignmentsMap[a.taskId] = a;
    }
  }
  const payload: PipelineState = {
    schemaVersion: 2,
    sessionId: session.sessionId,
    status: session.status,
    planDbId: session.planDbId ?? null,
    baseCwd: session.baseCwd ?? session.cwd,
    tasks: session.tasks.map((t) => ({
      id: t.id,
      title: t.title,
      state: session.taskStates[t.id] ?? "PENDING",
      assignedModel: t.assigned_model,
      turnId: session.taskTurnIds?.[t.id],
      startedAt: session.taskStartedAt?.[t.id],
      endedAt: session.taskEndedAt?.[t.id],
      provider: session.taskProviders?.[t.id] ?? null,
      threadId: session.threadIds?.[t.id] ?? null,
      cwd: session.taskCwds?.[t.id] ?? null,
      worktree: session.taskWorktrees?.[t.id] ?? null,
      repairStep: session.repairSteps?.[t.id] ?? null,
      allowedCommands: session.taskAllowed?.[t.id] ?? [],
      approvals: session.taskApprovals?.[t.id] ?? [],
      costUsd: session.taskCostUsd?.[t.id] ?? null,
    })),
    taskItems: session.tasks,
    contracts: session.contracts ?? [],
    upstreamArtifacts: session.upstreamArtifacts ?? {},
    assignments: assignmentsMap,
    deviationSources: session.deviationSources ?? [],
    integration: session.integration ?? null,
    taskStates: session.taskStates as Record<string, string>,
    taskLogs: session.taskLogs,
    gateResults: session.gateResults ?? [],
    startedAt: session.startedAt,
    updatedAt: Date.now(),
  };
  savePipelineState(session.cwd, payload).catch((err) => {
    console.error("Failed to persist pipeline state to disk:", err);
  });
}

function persistSessionDebounced(session: ActivePipelineSession, delayMs = 300) {
  if (!session.cwd) return;
  if (persistTimers[session.sessionId]) {
    clearTimeout(persistTimers[session.sessionId]);
  }
  persistTimers[session.sessionId] = setTimeout(() => {
    delete persistTimers[session.sessionId];
    persistSessionImmediate(session);
  }, delayMs);
}

const persistSession = persistSessionImmediate;

export const useExecution = create<ExecutionState>((set, get) => ({
  pipelines: {},

  startPlanExecution: (
    sessionId: string,
    tasks: TaskItem[],
    cwd?: string,
    contracts?: PlanContract[],
    assignments?: TaskAssignment[],
  ) => {
    const initialStates: Record<string, TaskState> = {};
    const initialLogs: Record<string, string[]> = {};
    const initialStartedAt: Record<string, number> = {};

    // Root tasks start READY for the dispatcher; RUNNING is claimed in dispatchSubAgentTask (F1.b).
    tasks.forEach((t) => {
      const isStartNode = t.depends_on.length === 0;
      initialStates[t.id] = isStartNode ? "READY" : "PENDING";
      initialLogs[t.id] = isStartNode
        ? [
            `[Ready] Awaiting dispatch`,
            `[Task goal] ${t.goal || t.title}`,
            `[Write scope] ${t.write_scope.join(", ") || "project root"}`,
          ]
        : [`[Suspended] Waiting for upstream dependencies: ${t.depends_on.join(", ")}`];
    });

    const session: ActivePipelineSession = {
      sessionId,
      status: "RUNNING",
      tasks,
      contracts,
      assignments,
      upstreamArtifacts: {},
      taskStates: initialStates,
      taskLogs: initialLogs,
      taskTurnIds: {},
      taskStartedAt: initialStartedAt,
      taskEndedAt: {},
      gateResults: [],
      threadIds: {},
      taskApprovals: {},
      taskAllowed: {},
      taskCwds: {},
      taskProviders: {},
      taskWindowConfigs: {},
      taskWorktrees: {},
      integration: null,
      taskAttemptCounts: {},
      repairSteps: {},
      taskCostUsd: {},
      deviationSources: [],
      taskWaitReasons: {},
      baseCwd: cwd,
      startedAt: Date.now(),
      cwd,
    };

    set((state) => ({
      pipelines: {
        ...state.pipelines,
        [sessionId]: session,
      },
    }));

    persistSession(session);
  },

  updateTaskState: (
    sessionId: string,
    taskId: string,
    nextState: TaskState,
    log?: string,
    turnId?: string,
  ) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;

      const prevState = current.taskStates[taskId] ?? "PENDING";
      if (!canTransition(prevState, nextState)) {
        const rejectMsg = `[State machine] Rejected illegal transition ${prevState}→${nextState}`;
        console.warn(rejectMsg, { sessionId, taskId });
        const currentLogs = current.taskLogs[taskId] ?? [];
        const updated: ActivePipelineSession = {
          ...current,
          taskLogs: {
            ...current.taskLogs,
            [taskId]: [...currentLogs, rejectMsg],
          },
        };
        persistSessionImmediate(updated);
        return {
          pipelines: {
            ...state.pipelines,
            [sessionId]: updated,
          },
        };
      }

      const nextStates = { ...current.taskStates, [taskId]: nextState };
      const currentLogs = current.taskLogs[taskId] ?? [];
      const nextLogs = {
        ...current.taskLogs,
        [taskId]: log ? [...currentLogs, log] : currentLogs,
      };

      const nextTurnIds = { ...(current.taskTurnIds ?? {}) };
      if (turnId) {
        nextTurnIds[taskId] = turnId;
      }

      const nextStartedAt = { ...(current.taskStartedAt ?? {}) };
      if (nextState === "RUNNING" && !nextStartedAt[taskId]) {
        nextStartedAt[taskId] = Date.now();
      }

      const nextEndedAt = { ...(current.taskEndedAt ?? {}) };
      if (nextState === "DONE" || nextState === "BLOCKED" || nextState === "CANCELLED") {
        nextEndedAt[taskId] = Date.now();
      }

      const updated: ActivePipelineSession = {
        ...current,
        taskStates: nextStates,
        taskLogs: nextLogs,
        taskTurnIds: nextTurnIds,
        taskStartedAt: nextStartedAt,
        taskEndedAt: nextEndedAt,
      };

      // Check if all tasks done
      const allDone = current.tasks.every((t) => nextStates[t.id] === "DONE");
      if (allDone) {
        updated.status = "DONE";
      }

      persistSessionImmediate(updated);

      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  claimTaskRunning: (sessionId, taskId, turnId, log) => {
    let claimed = false;
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const prev = current.taskStates[taskId];
      // F13.c: repair ladder also claims REPAIRING → RUNNING
      if (prev !== "READY" && prev !== "REPAIRING") return state;

      claimed = true;
      const currentLogs = current.taskLogs[taskId] ?? [];
      const updated: ActivePipelineSession = {
        ...current,
        taskStates: { ...current.taskStates, [taskId]: "RUNNING" },
        taskLogs: {
          ...current.taskLogs,
          [taskId]: log ? [...currentLogs, log] : currentLogs,
        },
        taskTurnIds: { ...(current.taskTurnIds ?? {}), [taskId]: turnId },
        taskStartedAt: {
          ...(current.taskStartedAt ?? {}),
          [taskId]: current.taskStartedAt?.[taskId] ?? Date.now(),
        },
      };
      persistSessionImmediate(updated);
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
    return claimed;
  },

  appendTaskLog: (sessionId: string, taskId: string, log: string) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;

      const currentLogs = current.taskLogs[taskId] ?? [];
      const nextLogs = {
        ...current.taskLogs,
        [taskId]: [...currentLogs, log],
      };

      const updated: ActivePipelineSession = {
        ...current,
        taskLogs: nextLogs,
      };

      persistSessionDebounced(updated, 300);

      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  recordTaskFailureEvidence: (sessionId, taskId, evidence) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const next = { ...(current.taskFailureEvidence ?? {}) };
      if (evidence) next[taskId] = evidence;
      else delete next[taskId];
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: { ...current, taskFailureEvidence: next },
        },
      };
    });
  },

  setTaskThreadId: (sessionId, taskId, threadId) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const updated: ActivePipelineSession = {
        ...current,
        threadIds: { ...(current.threadIds ?? {}), [taskId]: threadId },
      };
      persistSessionDebounced(updated, 300);
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
  },

  upsertTaskApproval: (sessionId, taskId, approval) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const list = [...(current.taskApprovals?.[taskId] ?? [])];
      const idx = list.findIndex((a) => a.id === approval.id);
      if (idx >= 0) list[idx] = approval;
      else list.push(approval);
      const updated: ActivePipelineSession = {
        ...current,
        taskApprovals: { ...(current.taskApprovals ?? {}), [taskId]: list },
      };
      persistSessionDebounced(updated, 300);
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
  },

  resolveTaskApproval: (sessionId, taskId, approvalId, decision) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const list = (current.taskApprovals?.[taskId] ?? []).map((a) =>
        a.id === approvalId ? { ...a, resolved: true, decision } : a,
      );
      const updated: ActivePipelineSession = {
        ...current,
        taskApprovals: { ...(current.taskApprovals ?? {}), [taskId]: list },
      };
      persistSessionDebounced(updated, 300);
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
  },

  addTaskAllowedCommand: (sessionId, taskId, command) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const prev = current.taskAllowed?.[taskId] ?? [];
      if (prev.includes(command)) return state;
      const updated: ActivePipelineSession = {
        ...current,
        taskAllowed: {
          ...(current.taskAllowed ?? {}),
          [taskId]: [...prev, command],
        },
      };
      persistSessionDebounced(updated, 300);
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
  },

  setTaskCwd: (sessionId, taskId, cwd) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const updated: ActivePipelineSession = {
        ...current,
        taskCwds: { ...(current.taskCwds ?? {}), [taskId]: cwd },
      };
      persistSessionDebounced(updated, 300);
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
  },

  setTaskProvider: (sessionId, taskId, provider) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const updated: ActivePipelineSession = {
        ...current,
        taskProviders: { ...(current.taskProviders ?? {}), [taskId]: provider },
      };
      persistSessionDebounced(updated, 300);
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
  },

  setTaskWindowConfig: (sessionId, taskId, config) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: {
            ...current,
            taskWindowConfigs: {
              ...(current.taskWindowConfigs ?? {}),
              [taskId]: config,
            },
          },
        },
      };
    });
  },

  setTaskWorktree: (sessionId, taskId, worktree) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const next = { ...(current.taskWorktrees ?? {}) };
      if (worktree) next[taskId] = worktree;
      else delete next[taskId];
      const updated: ActivePipelineSession = { ...current, taskWorktrees: next };
      persistSessionImmediate(updated);
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
  },

  setIntegration: (sessionId, integration) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const updated: ActivePipelineSession = { ...current, integration };
      persistSessionImmediate(updated);
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
  },

  setDelivery: (sessionId, delivery) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: { ...current, delivery },
        },
      };
    });
  },

  setPlanDbIds: (sessionId, planDbId, taskDbIds) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const updated: ActivePipelineSession = { ...current, planDbId, taskDbIds };
      persistSessionImmediate(updated);
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
  },

  nextTaskAttempt: (sessionId, taskId) => {
    let next = 1;
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      next = (current.taskAttemptCounts?.[taskId] ?? 0) + 1;
      const updated: ActivePipelineSession = {
        ...current,
        taskAttemptCounts: {
          ...(current.taskAttemptCounts ?? {}),
          [taskId]: next,
        },
      };
      return {
        pipelines: { ...state.pipelines, [sessionId]: updated },
      };
    });
    return next;
  },

  discardPipeline: (sessionId) => {
    set((state) => {
      const next = { ...state.pipelines };
      delete next[sessionId];
      return { pipelines: next };
    });
  },

  recordGateResult: (sessionId: string, result: PipelineGateResult) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;

      const gateResults = [...(current.gateResults ?? []), result];
      const updated: ActivePipelineSession = {
        ...current,
        gateResults,
      };

      persistSessionImmediate(updated);

      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  recordTaskArtifact: (sessionId: string, taskId: string, artifact: UpstreamArtifact) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;

      const nextArtifacts = { ...(current.upstreamArtifacts ?? {}), [taskId]: artifact };
      const updated: ActivePipelineSession = {
        ...current,
        upstreamArtifacts: nextArtifacts,
      };

      persistSessionImmediate(updated);

      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  pauseExecution: (sessionId: string) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const updated: ActivePipelineSession = { ...current, status: "PAUSED" };
      persistSession(updated);
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  resumeExecution: (sessionId: string) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const updated: ActivePipelineSession = { ...current, status: "RUNNING" };
      persistSession(updated);
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  updateTaskAssignedModel: (sessionId: string, taskId: string, modelSpec: string) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;

      const updatedTasks = current.tasks.map((t) =>
        t.id === taskId ? { ...t, assigned_model: modelSpec } : t,
      );

      const updated: ActivePipelineSession = {
        ...current,
        tasks: updatedTasks,
      };

      persistSessionImmediate(updated);

      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  setRepairStep: (sessionId, taskId, step) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const updated: ActivePipelineSession = {
        ...current,
        repairSteps: { ...(current.repairSteps ?? {}), [taskId]: step },
      };
      persistSessionImmediate(updated);
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  updatePipelineTasks: (sessionId: string, tasks: TaskItem[]) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;

      const updated: ActivePipelineSession = {
        ...current,
        tasks,
      };

      persistSessionImmediate(updated);

      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  setTaskWaitReasons: (sessionId: string, reasons: Record<string, unknown>) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: { ...current, taskWaitReasons: reasons },
        },
      };
    });
  },

  addDeviationSource: (sessionId: string, taskId: string) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const existing = current.deviationSources ?? [];
      if (existing.includes(taskId)) return state;
      const updated: ActivePipelineSession = {
        ...current,
        deviationSources: [...existing, taskId],
      };
      persistSessionImmediate(updated);
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  confirmDeviationSource: (sessionId: string, taskId: string) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      const updated: ActivePipelineSession = {
        ...current,
        deviationSources: (current.deviationSources ?? []).filter((id) => id !== taskId),
      };
      persistSessionImmediate(updated);
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: updated,
        },
      };
    });
  },

  loadPipelineFromDisk: async (cwd: string, sessionId: string) => {
    try {
      const loaded = await loadPipelineState(cwd, sessionId);
      if (!loaded) return null;

      // F7.e: INTERRUPTED marking is done once in Rust load_pipeline_state.
      const schemaVersion = loaded.schemaVersion ?? 1;
      const restoredStates = { ...(loaded.taskStates as Record<string, TaskState>) };
      const restoredLogs = { ...(loaded.taskLogs ?? {}) };

      const taskTurnIds: Record<string, string> = {};
      const taskStartedAt: Record<string, number> = {};
      const taskEndedAt: Record<string, number> = {};
      const threadIds: Record<string, string> = {};
      const taskCwds: Record<string, string> = {};
      const taskProviders: Record<string, string> = {};
      const taskWorktrees: Record<string, TaskWorktreeInfo> = {};
      const repairSteps: Record<string, string> = {};
      const taskAllowed: Record<string, string[]> = {};
      const taskApprovals: Record<string, TaskApproval[]> = {};
      const taskCostUsd: Record<string, number> = {};

      for (const t of loaded.tasks) {
        if (t.turnId) taskTurnIds[t.id] = t.turnId;
        if (t.startedAt) taskStartedAt[t.id] = t.startedAt;
        if (t.endedAt) taskEndedAt[t.id] = t.endedAt;
        if (t.threadId) threadIds[t.id] = t.threadId;
        if (t.cwd) taskCwds[t.id] = t.cwd;
        if (t.provider) taskProviders[t.id] = t.provider;
        if (t.worktree) taskWorktrees[t.id] = t.worktree;
        if (t.repairStep) repairSteps[t.id] = t.repairStep;
        if (t.allowedCommands?.length) taskAllowed[t.id] = [...t.allowedCommands];
        if (t.approvals?.length) {
          taskApprovals[t.id] = t.approvals as TaskApproval[];
        }
        if (typeof t.costUsd === "number") taskCostUsd[t.id] = t.costUsd;
        // Ensure taskStates has an entry for every task info row.
        if (!restoredStates[t.id] && t.state) {
          restoredStates[t.id] = t.state as TaskState;
        }
      }

      let tasks: TaskItem[] = [];
      let contracts: PlanContract[] = loaded.contracts ?? [];
      let unrecoverable = false;

      const v2Items = loaded.taskItems ?? [];
      if (schemaVersion >= 2 && v2Items.length > 0) {
        tasks = v2Items;
        if (!contracts.length) {
          // keep empty; v2 may omit when none
        }
      } else {
        // v1: backfill from persisted plan by task id.
        try {
          const plan = await loadPlan(cwd, sessionId);
          const byId = new Map(plan.tasks.map((t) => [t.id, t]));
          const missing = loaded.tasks.filter((t) => !byId.has(t.id));
          if (missing.length > 0 || plan.tasks.length === 0) {
            unrecoverable = true;
            tasks = loaded.tasks.map((t) => byId.get(t.id) ?? placeholderTask(t));
          } else {
            tasks = loaded.tasks.map((t) => {
              const full = byId.get(t.id)!;
              return {
                ...full,
                assigned_model: t.assignedModel ?? full.assigned_model,
              };
            });
            if (!contracts.length) contracts = plan.contracts ?? [];
          }
        } catch {
          unrecoverable = true;
          tasks = loaded.tasks.map((t) => placeholderTask(t));
        }
      }

      // Prefer persisted artifacts; fill gaps from SQLite task_reports when missing.
      const upstreamArtifacts: Record<string, UpstreamArtifact> = {
        ...(loaded.upstreamArtifacts ?? {}),
      };
      for (const t of tasks) {
        if (restoredStates[t.id] !== "DONE" || upstreamArtifacts[t.id]) continue;
        try {
          const rows = await getTaskReports(t.id);
          const latest = rows[rows.length - 1];
          if (!latest) continue;
          upstreamArtifacts[t.id] = {
            taskId: t.id,
            title: t.title,
            report: latest.summaryMd || latest.fullMd || "",
            changedFiles: latest.changedFilesList || [],
          };
        } catch {
          // SQLite may be empty; keep whatever we have.
        }
      }

      const assignmentsRecord = loaded.assignments ?? {};
      const assignments: TaskAssignment[] = Object.values(assignmentsRecord);

      const active: ActivePipelineSession = {
        sessionId: loaded.sessionId,
        status: loaded.status,
        tasks,
        contracts,
        assignments,
        upstreamArtifacts,
        taskStates: restoredStates,
        taskLogs: restoredLogs,
        taskTurnIds,
        taskStartedAt,
        taskEndedAt,
        gateResults: loaded.gateResults,
        threadIds,
        taskApprovals,
        taskAllowed,
        taskCwds,
        taskProviders,
        taskWindowConfigs: {},
        taskWorktrees,
        integration: loaded.integration ?? null,
        taskAttemptCounts: {},
        repairSteps,
        taskCostUsd,
        deviationSources: loaded.deviationSources ?? [],
        planDbId: loaded.planDbId ?? null,
        baseCwd: loaded.baseCwd ?? cwd,
        unrecoverable,
        startedAt: loaded.startedAt,
        cwd,
      };

      set((state) => ({
        pipelines: {
          ...state.pipelines,
          [sessionId]: active,
        },
      }));

      return active;
    } catch (err) {
      console.warn("Failed to load pipeline state from disk:", err);
      return null;
    }
  },

  getPipeline: (sessionId: string) => get().pipelines[sessionId],
}));
