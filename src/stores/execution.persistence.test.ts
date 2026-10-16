import { beforeEach, describe, expect, it, vi } from "vitest";
import * as ipc from "../lib/ipc";
import { checkpointPipeline, flushPipelines, retryPipelinePersistence, useExecution } from "./execution";
import type { TaskItem } from "../lib/ipc";

const task: TaskItem = {
  id: "T1", title: "persist", domain: "backend", goal: "test", depends_on: [],
  contracts: [], write_scope: [], deliverables: [], acceptance: { commands: [], criteria: [] },
  size: "M", difficulty: 2,
};
let sequence = 0;
function start() {
  const id = `persistence-test-${++sequence}`;
  useExecution.getState().startPlanExecution(id, [task], "/fixture");
  return id;
}
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

beforeEach(async () => {
  await flushPipelines();
  useExecution.setState({ pipelines: {}, closing: false });
  vi.restoreAllMocks();
});

describe("pipeline persistence", () => {
  it("serializes same-session requests, snapshots immediately, and flush waits for in-flight saves", async () => {
    const first = deferred();
    const saved: ipc.PipelineState[] = [];
    const save = vi.spyOn(ipc, "savePipelineState").mockImplementation(async (_cwd, state) => {
      saved.push(state);
      if (saved.length === 1) await first.promise;
    });
    const id = start();
    await Promise.resolve();
    useExecution.getState().updateTaskState(id, "T1", "RUNNING", undefined, "turn-1");
    useExecution.getState().updateTaskState(id, "T1", "VERIFYING");
    useExecution.getState().updateTaskState(id, "T1", "DONE");
    let flushed = false;
    const flushing = flushPipelines().then(() => { flushed = true; });
    await Promise.resolve();
    expect(save).toHaveBeenCalledTimes(1);
    expect(saved[0].taskStates.T1).toBe("READY");
    expect(flushed).toBe(false);
    first.resolve();
    await flushing;
    expect(saved.at(-1)?.taskStates.T1).toBe("DONE");
    expect(saved.map((s) => s.persistenceRevision)).toEqual([...saved.keys()].map((i) => i + 1));
  });

  it("exposes failures, blocks claims/checkpoints, retains latest state, and retries without resuming", async () => {
    const save = vi.spyOn(ipc, "savePipelineState").mockRejectedValue(new Error("disk full"));
    const id = start();
    await expect(checkpointPipeline(id)).rejects.toThrow("disk full");
    const store = useExecution.getState();
    expect(store.pipelines[id].status).toBe("PAUSED");
    expect(store.pipelines[id].persistenceError).toContain("disk full");
    expect(store.claimTaskRunning(id, "T1", "must-not-start")).toBe(false);
    store.resumeExecution(id);
    expect(useExecution.getState().pipelines[id].status).toBe("PAUSED");
    await expect(flushPipelines()).rejects.toThrow("disk full");
    store.appendTaskLog(id, "T1", "latest log survives retry");
    save.mockResolvedValue(undefined);
    await retryPipelinePersistence(id);
    expect(useExecution.getState().pipelines[id].persistenceError).toBeUndefined();
    expect(useExecution.getState().pipelines[id].status).toBe("PAUSED");
    expect(save.mock.calls.at(-1)?.[1].taskLogs.T1).toContain("latest log survives retry");
    await checkpointPipeline(id);
  });

  it("flushes debounced metadata and seeds revisions from a recovered snapshot", async () => {
    const save = vi.spyOn(ipc, "savePipelineState").mockResolvedValue(undefined);
    const id = start();
    await checkpointPipeline(id);
    useExecution.setState((s) => ({ pipelines: {
      ...s.pipelines, [id]: { ...s.pipelines[id], persistenceRevision: 100 },
    } }));
    useExecution.getState().setTaskCwd(id, "T1", "/fixture/worktree");
    await flushPipelines();
    expect(save.mock.calls.at(-1)?.[1].tasks[0].cwd).toBe("/fixture/worktree");
    expect(save.mock.calls.at(-1)?.[1].persistenceRevision).toBe(101);
  });
  it("drains metadata added during a flush and blocks new work while closing", async () => {
    const write = deferred();
    const save = vi.spyOn(ipc, "savePipelineState").mockResolvedValue(undefined);
    const id = start();
    await checkpointPipeline(id);
    save.mockImplementationOnce(() => write.promise);
    const flushing = flushPipelines();
    await Promise.resolve();
    useExecution.getState().setTaskThreadId(id, "T1", "arrived-during-flush");
    useExecution.setState({ closing: true });
    expect(useExecution.getState().claimTaskRunning(id, "T1", "must-not-start")).toBe(false);
    await expect(checkpointPipeline(id)).rejects.toThrow("Window is closing");
    write.resolve();
    await flushing;
    expect(save.mock.calls.at(-1)?.[1].tasks[0].threadId).toBe("arrived-during-flush");
    useExecution.setState({ closing: false });
  });

  it("does not block another session behind a slow session's save", async () => {
    const slow = deferred();
    const save = vi.spyOn(ipc, "savePipelineState").mockImplementation(async (_cwd, state) => {
      if (state.sessionId === "slow-save") await slow.promise;
    });
    useExecution.getState().startPlanExecution("slow-save", [task], "/fixture");
    const fast = start();
    await checkpointPipeline(fast);
    expect(save.mock.calls.some(([, snapshot]) => snapshot.sessionId === fast)).toBe(true);
    slow.resolve();
    await flushPipelines();
  });

});
