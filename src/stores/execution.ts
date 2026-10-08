import { create } from "zustand";
import {
  loadPipelineState,
  savePipelineState,
  type PipelineGateResult,
  type PipelineState,
  type PipelineStatus,
  type PlanContract,
  type TaskItem,
  type TaskState,
  type UpstreamArtifact,
} from "../lib/ipc";

export interface ActivePipelineSession {
  sessionId: string;
  status: PipelineStatus;
  tasks: TaskItem[];
  contracts?: PlanContract[];
  upstreamArtifacts?: Record<string, UpstreamArtifact>;
  taskStates: Record<string, TaskState>;
  taskLogs: Record<string, string[]>;
  taskTurnIds?: Record<string, string>;
  taskStartedAt?: Record<string, number>;
  taskEndedAt?: Record<string, number>;
  gateResults?: PipelineGateResult[];
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
  ) => void;
  updateTaskState: (
    sessionId: string,
    taskId: string,
    state: TaskState,
    log?: string,
    turnId?: string,
  ) => void;
  appendTaskLog: (sessionId: string, taskId: string, log: string) => void;
  recordGateResult: (sessionId: string, result: PipelineGateResult) => void;
  recordTaskArtifact: (sessionId: string, taskId: string, artifact: UpstreamArtifact) => void;
  pauseExecution: (sessionId: string) => void;
  resumeExecution: (sessionId: string) => void;
  updateTaskAssignedModel: (sessionId: string, taskId: string, modelSpec: string) => void;
  updatePipelineTasks: (sessionId: string, tasks: TaskItem[]) => void;
  loadPipelineFromDisk: (cwd: string, sessionId: string) => Promise<ActivePipelineSession | null>;
  getPipeline: (sessionId: string) => ActivePipelineSession | undefined;
}

const persistTimers: Record<string, ReturnType<typeof setTimeout>> = {};

function persistSessionImmediate(session: ActivePipelineSession) {
  if (!session.cwd) return;
  if (persistTimers[session.sessionId]) {
    clearTimeout(persistTimers[session.sessionId]);
    delete persistTimers[session.sessionId];
  }
  const payload: PipelineState = {
    sessionId: session.sessionId,
    status: session.status,
    tasks: session.tasks.map((t) => ({
      id: t.id,
      title: t.title,
      state: session.taskStates[t.id] ?? "PENDING",
      assignedModel: t.assigned_model,
      turnId: session.taskTurnIds?.[t.id],
      startedAt: session.taskStartedAt?.[t.id],
      endedAt: session.taskEndedAt?.[t.id],
    })),
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
            `[就绪] 等待调度`,
            `[任务目标] ${t.goal || t.title}`,
            `[工作范围] ${t.write_scope.join(", ") || "项目根目录"}`,
          ]
        : [`[挂起] 等待前置依赖任务完成: ${t.depends_on.join(", ")}`];
    });

    const session: ActivePipelineSession = {
      sessionId,
      status: "RUNNING",
      tasks,
      contracts,
      upstreamArtifacts: {},
      taskStates: initialStates,
      taskLogs: initialLogs,
      taskTurnIds: {},
      taskStartedAt: initialStartedAt,
      taskEndedAt: {},
      gateResults: [],
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

  loadPipelineFromDisk: async (cwd: string, sessionId: string) => {
    try {
      const loaded = await loadPipelineState(cwd, sessionId);
      if (!loaded) return null;

      const tasks: TaskItem[] = loaded.tasks.map((t) => ({
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
      }));

      const taskTurnIds: Record<string, string> = {};
      const taskStartedAt: Record<string, number> = {};
      const taskEndedAt: Record<string, number> = {};
      const restoredStates = { ...(loaded.taskStates as Record<string, TaskState>) };
      const restoredLogs = { ...(loaded.taskLogs ?? {}) };

      let hasInterrupted = false;
      loaded.tasks.forEach((t) => {
        if (t.turnId) taskTurnIds[t.id] = t.turnId;
        if (t.startedAt) taskStartedAt[t.id] = t.startedAt;
        if (t.endedAt) taskEndedAt[t.id] = t.endedAt;

        // Cold-start / crash recovery (m2c & m5c):
        // If a task was marked RUNNING or VERIFYING when the process exited, mark it INTERRUPTED
        if (restoredStates[t.id] === "RUNNING" || restoredStates[t.id] === "VERIFYING") {
          restoredStates[t.id] = "INTERRUPTED";
          hasInterrupted = true;
          const prevLogs = restoredLogs[t.id] ?? [];
          restoredLogs[t.id] = [
            ...prevLogs,
            `[系统警告] 进程异常退出/崩溃中断，子代理任务已置为 INTERRUPTED，等待手动恢复。`,
          ];
        }
      });

      const restoredStatus: PipelineStatus =
        loaded.status === "RUNNING" && hasInterrupted ? "INTERRUPTED" : loaded.status;

      const active: ActivePipelineSession = {
        sessionId: loaded.sessionId,
        status: restoredStatus,
        tasks,
        taskStates: restoredStates,
        taskLogs: restoredLogs,
        taskTurnIds,
        taskStartedAt,
        taskEndedAt,
        gateResults: loaded.gateResults,
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
