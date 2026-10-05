import { create } from "zustand";
import type { TaskItem, TaskState } from "../lib/ipc";

export interface ActivePipelineSession {
  sessionId: string;
  status: "RUNNING" | "PAUSED" | "DONE";
  tasks: TaskItem[];
  taskStates: Record<string, TaskState>;
  taskLogs: Record<string, string[]>;
  startedAt: number;
}

interface ExecutionState {
  // sessionId -> ActivePipelineSession
  pipelines: Record<string, ActivePipelineSession>;
  startPlanExecution: (sessionId: string, tasks: TaskItem[]) => void;
  updateTaskState: (sessionId: string, taskId: string, state: TaskState, log?: string) => void;
  pauseExecution: (sessionId: string) => void;
  resumeExecution: (sessionId: string) => void;
  getPipeline: (sessionId: string) => ActivePipelineSession | undefined;
}

export const useExecution = create<ExecutionState>((set, get) => ({
  pipelines: {},

  startPlanExecution: (sessionId: string, tasks: TaskItem[]) => {
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

    set((state) => ({
      pipelines: {
        ...state.pipelines,
        [sessionId]: {
          sessionId,
          status: "RUNNING",
          tasks,
          taskStates: initialStates,
          taskLogs: initialLogs,
          startedAt: Date.now(),
        },
      },
    }));
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

      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: {
            ...current,
            taskStates: nextStates,
            taskLogs: nextLogs,
          },
        },
      };
    });
  },

  pauseExecution: (sessionId: string) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: { ...current, status: "PAUSED" },
        },
      };
    });
  },

  resumeExecution: (sessionId: string) => {
    set((state) => {
      const current = state.pipelines[sessionId];
      if (!current) return state;
      return {
        pipelines: {
          ...state.pipelines,
          [sessionId]: { ...current, status: "RUNNING" },
        },
      };
    });
  },

  getPipeline: (sessionId: string) => get().pipelines[sessionId],
}));
