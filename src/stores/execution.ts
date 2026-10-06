import { create } from "zustand";
import {
  loadPipelineState,
  savePipelineState,
  type PipelineGateResult,
  type PipelineState,
  type PipelineStatus,
  type TaskItem,
  type TaskState,
} from "../lib/ipc";

export interface ActivePipelineSession {
  sessionId: string;
  status: PipelineStatus;
  tasks: TaskItem[];
  taskStates: Record<string, TaskState>;
  taskLogs: Record<string, string[]>;
  gateResults?: PipelineGateResult[];
  startedAt: number;
  cwd?: string;
}

interface ExecutionState {
  // sessionId -> ActivePipelineSession
  pipelines: Record<string, ActivePipelineSession>;
  startPlanExecution: (sessionId: string, tasks: TaskItem[], cwd?: string) => void;
  updateTaskState: (sessionId: string, taskId: string, state: TaskState, log?: string) => void;
  recordGateResult: (sessionId: string, result: PipelineGateResult) => void;
  pauseExecution: (sessionId: string) => void;
  resumeExecution: (sessionId: string) => void;
  loadPipelineFromDisk: (cwd: string, sessionId: string) => Promise<ActivePipelineSession | null>;
  getPipeline: (sessionId: string) => ActivePipelineSession | undefined;
}

function persistSession(session: ActivePipelineSession) {
  if (!session.cwd) return;
  const payload: PipelineState = {
    sessionId: session.sessionId,
    status: session.status,
    tasks: session.tasks.map((t) => ({
      id: t.id,
      title: t.title,
      state: session.taskStates[t.id] ?? "PENDING",
      assignedModel: t.assigned_model,
      turnId: undefined,
      startedAt: undefined,
      endedAt: undefined,
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

export const useExecution = create<ExecutionState>((set, get) => ({
  pipelines: {},

  startPlanExecution: (sessionId: string, tasks: TaskItem[], cwd?: string) => {
    const initialStates: Record<string, TaskState> = {};
    const initialLogs: Record<string, string[]> = {};

    // Topological starter: tasks with no dependencies start RUNNING in parallel
    tasks.forEach((t) => {
      const isStartNode = t.depends_on.length === 0;
      initialStates[t.id] = isStartNode ? "RUNNING" : "PENDING";
      initialLogs[t.id] = isStartNode
        ? [
            `[系统] 子代理启动，指派模型进行独立环境探查与代码实现...`,
            `[任务目标] ${t.goal || t.title}`,
            `[工作范围] ${t.write_scope.join(", ") || "项目根目录"}`,
          ]
        : [`[挂起] 等待前置依赖任务完成: ${t.depends_on.join(", ")}`];
    });

    const session: ActivePipelineSession = {
      sessionId,
      status: "RUNNING",
      tasks,
      taskStates: initialStates,
      taskLogs: initialLogs,
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

  updateTaskState: (sessionId: string, taskId: string, nextState: TaskState, log?: string) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;

      const nextStates = { ...current.taskStates, [taskId]: nextState };
      const currentLogs = current.taskLogs[taskId] ?? [];
      const nextLogs = {
        ...current.taskLogs,
        [taskId]: log ? [...currentLogs, log] : currentLogs,
      };

      const updated: ActivePipelineSession = {
        ...current,
        taskStates: nextStates,
        taskLogs: nextLogs,
      };

      // Check if all tasks done
      const allDone = current.tasks.every((t) => nextStates[t.id] === "DONE");
      if (allDone) {
        updated.status = "DONE";
      }

      persistSession(updated);

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

      persistSession(updated);

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

      const active: ActivePipelineSession = {
        sessionId: loaded.sessionId,
        status: loaded.status,
        tasks,
        taskStates: loaded.taskStates as Record<string, TaskState>,
        taskLogs: loaded.taskLogs,
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
