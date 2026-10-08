import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  getLastDirectPrefer,
  peekPendingTaskFollowUp,
  recordLastDirectTurn,
  routePipelineAppend,
  takePendingTaskFollowUp,
} from "./appendTriage";

vi.mock("./ipc", async () => {
  const actual = await vi.importActual<typeof import("./ipc")>("./ipc");
  return {
    ...actual,
    triageMainSessionMessage: vi.fn(),
    loadPlan: vi.fn(),
  };
});

vi.mock("../stores/execution", () => ({
  useExecution: {
    getState: vi.fn(),
  },
}));

vi.mock("../stores/chat", () => ({
  useChat: {
    getState: () => ({
      postSystemMessage: vi.fn(),
    }),
  },
}));

vi.mock("../stores/projects", () => ({
  useProjects: {
    getState: () => ({ projects: [] }),
  },
  sessionCwd: vi.fn(),
}));

vi.mock("./replan", () => ({
  canReplan: vi.fn(() => "ok"),
  startReplanTurn: vi.fn(),
}));

import { triageMainSessionMessage } from "./ipc";
import { useExecution } from "../stores/execution";

describe("appendTriage (F16.e)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("records last direct prefer for previous-window routing", () => {
    recordLastDirectTurn("s1", { provider: "CLAUDE_CODE", model: "sonnet" });
    expect(getLastDirectPrefer("s1")).toEqual({
      ref: { provider: "CLAUDE_CODE", model: "sonnet" },
      mode: "direct",
    });
  });

  it("queues task follow-up when triage routes to TASK_EXECUTION_WINDOW", async () => {
    const appendTaskLog = vi.fn();
    (useExecution.getState as ReturnType<typeof vi.fn>).mockReturnValue({
      pipelines: {
        s1: {
          status: "RUNNING",
          tasks: [
            {
              id: "T1",
              title: "Setup migrations",
              write_scope: ["src/db/"],
            },
          ],
          taskStates: { T1: "RUNNING" },
        },
      },
      appendTaskLog,
    });
    (triageMainSessionMessage as ReturnType<typeof vi.fn>).mockResolvedValue({
      type: "TASK_EXECUTION_WINDOW",
      taskId: "T1",
      taskTitle: "Setup migrations",
      reason: "Mentioned task ID T1",
    });

    const result = await routePipelineAppend("s1", "T1 把字段改成 INTEGER");
    expect(result).toEqual({
      action: "handled",
      reason: "Mentioned task ID T1",
    });
    expect(peekPendingTaskFollowUp("s1", "T1")).toBe("T1 把字段改成 INTEGER");
    expect(takePendingTaskFollowUp("s1", "T1")).toBe("T1 把字段改成 INTEGER");
    expect(peekPendingTaskFollowUp("s1", "T1")).toBeNull();
    expect(appendTaskLog).toHaveBeenCalled();
  });

  it("returns direct action for DIRECT_DISPATCH", async () => {
    (useExecution.getState as ReturnType<typeof vi.fn>).mockReturnValue({
      pipelines: {
        s1: { status: "RUNNING", tasks: [], taskStates: {} },
      },
      appendTaskLog: vi.fn(),
    });
    (triageMainSessionMessage as ReturnType<typeof vi.fn>).mockResolvedValue({
      type: "DIRECT_DISPATCH",
      fresh: true,
      reason: "standalone",
    });

    const result = await routePipelineAppend("s1", "什么是 RepoDigest？");
    expect(result).toEqual({
      action: "direct",
      preferPrevious: false,
      reason: "standalone",
    });
  });

  it("returns null when no pipeline", async () => {
    (useExecution.getState as ReturnType<typeof vi.fn>).mockReturnValue({
      pipelines: {},
      appendTaskLog: vi.fn(),
    });
    expect(await routePipelineAppend("s1", "hi")).toBeNull();
  });
});
