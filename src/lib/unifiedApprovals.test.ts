import { describe, expect, it } from "vitest";
import { collectUnifiedApprovals } from "./unifiedApprovals";
import type { ActivePipelineSession } from "../stores/execution";

function basePipeline(
  overrides: Partial<ActivePipelineSession> = {},
): ActivePipelineSession {
  return {
    sessionId: "s1",
    status: "RUNNING",
    tasks: [],
    taskStates: {},
    taskLogs: {},
    startedAt: 1,
    ...overrides,
  };
}

describe("collectUnifiedApprovals", () => {
  it("flattens unresolved approvals oldest-first", () => {
    const pipeline = basePipeline({
      taskApprovals: {
        T2: [
          {
            id: "a2",
            turnId: "t2",
            toolId: null,
            command: "rm -rf /tmp/x",
            level: "danger",
            reason: "destructive",
            recoverable: null,
            resolved: false,
            createdAtMs: 200,
          },
        ],
        T1: [
          {
            id: "a1",
            turnId: "t1",
            toolId: null,
            command: "npm install",
            level: "warn",
            reason: "network",
            recoverable: null,
            resolved: false,
            createdAtMs: 100,
          },
          {
            id: "a0",
            turnId: "t0",
            toolId: null,
            command: "done",
            level: "info",
            reason: "ok",
            recoverable: null,
            resolved: true,
            createdAtMs: 50,
          },
        ],
      },
    });
    const items = collectUnifiedApprovals(pipeline);
    expect(items.map((i) => i.approvalId)).toEqual(["a1", "a2"]);
    expect(items[0].taskId).toBe("T1");
    expect(items[1].taskId).toBe("T2");
  });

  it("returns empty when no pipeline", () => {
    expect(collectUnifiedApprovals(null)).toEqual([]);
  });
});
