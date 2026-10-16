import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as ipc from "./ipc";
import { checkpointPipeline, flushPipelines, retryPipelinePersistence, useExecution } from "../stores/execution";
import { useSettings } from "../stores/settings";
import { continueTask, completeTakeoverAndRerunGates, processSubAgentCompletion, resetSubAgentEventListener, dispatchReadyTasks } from "./taskRunner";

const task: ipc.TaskItem = { id: "T1", title: "delivery", goal: "test", domain: "backend", depends_on: [],
  contracts: [], write_scope: ["a"], deliverables: [], acceptance: { commands: ["test"], criteria: [] }, size: "S", difficulty: 1 };
const next = { ...task, id: "T2", depends_on: ["T1"] };
const wt = { root: "/wt/task", cwd: "/wt/task", branch: "tokage/task", baseCommit: "base" };
const integration = { root: "/wt/int", cwd: "/wt/int", branch: "tokage/int", baseCommit: "base" };
const snapshot: ipc.ScopeSnapshot = { root: wt.root, workdir: wt.cwd, prefix: "", head: "base", clean: false, files: { a: { disk: "accepted", index: "old" } } };
const ctx = () => ({ sessionId: "s", taskId: "T1", task, worktreeCwd: wt.cwd, turnId: "turn", rawOutput: "done", changedFiles: [] as string[], toolCalls: [], startTime: 1 });
const pipeline = () => useExecution.getState().pipelines.s;
const conflictTask: ipc.TaskItem = { ...task, id: "T1-integration", domain: "integration", depends_on: ["T1"] };
const conflict: ipc.WorktreeMergeOutcome = { conflict: { conflictedFiles: ["a"], integrationTask: conflictTask } };
let saved: ipc.PipelineState[];

beforeEach(async () => {
  await flushPipelines(); vi.restoreAllMocks(); resetSubAgentEventListener();
  useExecution.setState({ pipelines: {}, closing: false }); useSettings.setState({ isolation: "worktree" });
  saved = [];
  vi.spyOn(ipc, "savePipelineState").mockImplementation(async (_cwd, state) => { saved.push(structuredClone(state)); });
  vi.spyOn(ipc, "gitScopeSnapshot").mockResolvedValue(snapshot);
  vi.spyOn(ipc, "validateWriteScope").mockImplementation(async (scopes, paths) => ({ passed: true, declaredScopes: scopes, changedFiles: paths, allowedFiles: paths, outOfScopeFiles: [] }));
  vi.spyOn(ipc, "executeGateCommand").mockResolvedValue({ command: "test", passed: true, durationMs: 1, outputTail: "ok" });
  vi.spyOn(ipc, "recordTaskReport").mockResolvedValue(1);
  vi.spyOn(ipc, "commitTaskWorktree").mockResolvedValue(true);
  vi.spyOn(ipc, "gitRevParse").mockResolvedValue("task-sha");
  vi.spyOn(ipc, "mergeTaskWorktree").mockResolvedValue({ success: { mergedBranch: integration.branch } });
  vi.spyOn(ipc, "removeTaskWorktree").mockResolvedValue(undefined);
  vi.spyOn(ipc, "createSessionWorktree").mockResolvedValue({ root: "/wt/resolver", cwd: "/wt/resolver", branch: "tokage/resolver" });
  vi.spyOn(ipc, "prepareIntegrationWorktree").mockResolvedValue({ root: "/wt/resolver", cwd: "/wt/resolver", branch: "tokage/resolver" });
  vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);
  vi.spyOn(ipc, "pickDispatchableTasks").mockResolvedValue({ promoted: [], dispatchable: [], ineligible: {} });
  useExecution.getState().startPlanExecution("s", [task, next], "/repo");
  useExecution.getState().claimTaskRunning("s", "T1", "turn");
  useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, planDbId: 1, integration,
    taskWorktrees: { T1: wt }, taskCwds: { T1: wt.cwd },
    scopeBaselines: { T1: { ...snapshot, clean: true, files: { a: { disk: "old", index: "old" } }, executionMode: "worktree" } },
  } } }));
});
afterEach(async () => { await flushPipelines(); resetSubAgentEventListener(); });

/** Simulate a cold load of an actually persisted checkpoint, not in-memory state. */
async function reload(state: ipc.PipelineState) {
  await flushPipelines(); resetSubAgentEventListener(); useExecution.setState({ pipelines: {} });
  const loaded = structuredClone(state);
  loaded.status = "INTERRUPTED"; loaded.taskStates.T1 = "INTERRUPTED";
  loaded.tasks.find((t) => t.id === "T1")!.state = "INTERRUPTED";
  vi.spyOn(ipc, "loadPipelineState").mockResolvedValue(loaded);
  await useExecution.getState().loadPipelineFromDisk("/repo", "s");
  expect(pipeline().gitDeliveries).toEqual(state.gitDeliveries);
  vi.mocked(ipc.commitTaskWorktree).mockClear(); vi.mocked(ipc.mergeTaskWorktree).mockClear();
  vi.mocked(ipc.startChatTurn).mockClear(); vi.mocked(ipc.executeGateCommand).mockClear();
}

describe("m1d Git delivery recovery", () => {
  it("persists intent before commit and merge outcome before non-forced cleanup", async () => {
    vi.mocked(ipc.commitTaskWorktree).mockImplementation(async () => {
      expect(saved.at(-1)?.gitDeliveries?.T1).toMatchObject({ phase: "committing", scope: { snapshot, changedFiles: ["a"] }, worktree: wt }); return true;
    });
    vi.mocked(ipc.mergeTaskWorktree).mockImplementation(async (...args) => {
      expect(saved.at(-1)?.gitDeliveries?.T1).toMatchObject({ phase: "committed", sourceCommit: "task-sha" });
      expect(args[4]).toBe("task-sha"); return { success: { mergedBranch: integration.branch } };
    });
    vi.mocked(ipc.removeTaskWorktree).mockImplementation(async (_root, _branch, keep, force) => {
      expect(saved.at(-1)?.gitDeliveries?.T1?.phase).toBe("merged"); expect(keep).toBe(true); expect(force).toBe(false);
    });
    await processSubAgentCompletion(ctx());
    expect(pipeline().taskStates.T1).toBe("DONE"); expect(pipeline().taskWorktrees?.T1).toBeUndefined();
  });

  it.each(["committing", "committed", "merged", "cleaned"])("recovers persisted %s checkpoint without a model turn", async (point) => {
    await processSubAgentCompletion(ctx()); await flushPipelines();
    const phase = point === "cleaned" ? "merged" : point;
    const state = point === "cleaned" ? [...saved].reverse().find((s) => s.gitDeliveries?.T1?.phase === "merged" && !s.tasks.find((t) => t.id === "T1")?.worktree)!
      : saved.find((s) => s.gitDeliveries?.T1?.phase === phase)!;
    expect(state).toBeTruthy();
    await reload(state);
    // Native commit reconciliation handles an already committed exact accepted tree.
    vi.mocked(ipc.commitTaskWorktree).mockResolvedValue(false);
    await continueTask("s", "T1");
    expect(pipeline().taskStates.T1).toBe("DONE");
    expect(ipc.commitTaskWorktree).toHaveBeenCalledTimes(point === "committing" ? 1 : 0);
    expect(ipc.mergeTaskWorktree).toHaveBeenCalledOnce();
    expect(ipc.startChatTurn).not.toHaveBeenCalled(); expect(ipc.executeGateCommand).not.toHaveBeenCalled();
  });

  it.each(["index.lock", "permission denied", "hook refused", "timed out", "missing branch"])("blocks %s without fabricating integration tasks or deleting work", async (failure) => {
    const reason = `git merge failure: ${failure === "timed out" ? "details ".repeat(70) : ""}${failure}`;
    vi.mocked(ipc.mergeTaskWorktree).mockRejectedValue(new Error(reason));
    await processSubAgentCompletion(ctx());
    expect(pipeline().taskStates.T1).toBe("BLOCKED"); expect(pipeline().tasks).toHaveLength(2);
    expect(pipeline().gitDeliveries?.T1?.phase).toBe("committed"); expect(pipeline().taskWorktrees?.T1).toEqual(wt);
    expect(ipc.removeTaskWorktree).not.toHaveBeenCalled(); expect(ipc.startChatTurn).not.toHaveBeenCalled();
    expect(pipeline().taskLogs.T1.join("\n")).toContain(reason);
    await flushPipelines();
    expect(saved[saved.length - 1].taskLogs.T1.join("\n")).toContain(reason);
    vi.mocked(ipc.mergeTaskWorktree).mockResolvedValue({ success: { mergedBranch: integration.branch } });
    expect(await completeTakeoverAndRerunGates("s", "T1")).toBe(true);
    expect(ipc.commitTaskWorktree).toHaveBeenCalledOnce(); // accepted commit reused
  });

  it("retains source and blocks when conflict preparation fails; retries the same persisted resolver", async () => {
    vi.mocked(ipc.mergeTaskWorktree).mockResolvedValue(conflict);
    vi.mocked(ipc.prepareIntegrationWorktree).mockRejectedValueOnce(new Error("pre-merge permission failure"));
    await processSubAgentCompletion(ctx());
    expect(pipeline().taskStates.T1).toBe("BLOCKED"); expect(pipeline().taskStates[conflictTask.id]).toBe("PENDING");
    expect(pipeline().tasks.find((t) => t.id === "T2")?.depends_on).toEqual(["T1", conflictTask.id]);
    expect(pipeline().taskWorktrees?.T1).toEqual(wt); expect(ipc.removeTaskWorktree).not.toHaveBeenCalled();
    expect(pipeline().gitDeliveries?.T1.preparation?.root).toBe("/wt/resolver");
    await flushPipelines();
    await reload(saved[saved.length - 1]);
    await continueTask("s", "T1");
    expect(pipeline().taskStates.T1).toBe("DONE");
    expect(ipc.createSessionWorktree).toHaveBeenCalledOnce(); expect(ipc.mergeTaskWorktree).not.toHaveBeenCalled();
    expect(pipeline().taskWorktrees?.[conflictTask.id]?.baseCommit).toBe("task-sha");
    expect(pipeline().tasks.filter((t) => t.id === conflictTask.id)).toHaveLength(1);
  });

  it.each(["source changed", "integration missing", "worktree mismatched"])("blocks recovery when %s", async (reason) => {
    await processSubAgentCompletion(ctx()); await reload(saved.find((s) => s.gitDeliveries?.T1?.phase === "committed")!);
    if (reason === "source changed") vi.mocked(ipc.gitRevParse).mockResolvedValue("someone-else");
    if (reason === "integration missing") useExecution.getState().setIntegration("s", null);
    if (reason === "worktree mismatched") useExecution.getState().setTaskWorktree("s", "T1", { ...wt, root: "/other" });
    await continueTask("s", "T1");
    expect(pipeline().taskStates.T1).toBe("BLOCKED"); expect(ipc.mergeTaskWorktree).not.toHaveBeenCalled();
    expect(ipc.startChatTurn).not.toHaveBeenCalled();
  });

  it("routes the board retry through Git reconciliation even without a runnable model", async () => {
    vi.mocked(ipc.mergeTaskWorktree).mockRejectedValueOnce(new Error("temporary lock"));
    await processSubAgentCompletion(ctx());
    useExecution.getState().updateTaskState("s", "T1", "READY");
    await dispatchReadyTasks("s", { only: ["T1"] });
    expect(pipeline().taskStates.T1).toBe("DONE");
    expect(ipc.commitTaskWorktree).toHaveBeenCalledOnce(); expect(ipc.startChatTurn).not.toHaveBeenCalled();
  });

  it("retains a dirty worktree when cleanup fails after confirmed merge", async () => {
    vi.mocked(ipc.removeTaskWorktree).mockRejectedValue(new Error("DIRTY"));
    await processSubAgentCompletion(ctx());
    expect(pipeline().taskStates.T1).toBe("DONE"); expect(pipeline().taskWorktrees?.T1).toEqual(wt);
    expect(pipeline().gitDeliveries?.T1?.phase).toBe("merged");
  });

  it.each(["committed", "merged"])("does not clean after a failed save of %s result", async (phase) => {
    await checkpointPipeline("s");
    vi.mocked(ipc.savePipelineState).mockImplementation(async (_cwd, state) => {
      if (state.gitDeliveries?.T1?.phase === phase) throw new Error("disk full after Git mutation");
      saved.push(structuredClone(state));
    });
    await processSubAgentCompletion(ctx());
    expect(pipeline().taskStates.T1).toBe("BLOCKED"); expect(ipc.mergeTaskWorktree).toHaveBeenCalledTimes(phase === "committed" ? 0 : 1);
    expect(ipc.removeTaskWorktree).not.toHaveBeenCalled();
    vi.mocked(ipc.savePipelineState).mockResolvedValue(undefined); await retryPipelinePersistence("s");
  });
});
