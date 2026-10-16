import { beforeEach, describe, expect, it, vi } from "vitest";
import * as ipc from "./ipc";
import { restoreTaskChanges } from "./taskRollback";
import { prepareScopeBaseline, releaseScopeOwner, resetScopeOwners } from "./scopeGuard";
import { flushPipelines, useExecution } from "../stores/execution";

const snapshot: ipc.ScopeSnapshot = { root: "/repo", workdir: "/repo", prefix: "", head: "base", clean: true, files: {} };
const task: ipc.TaskItem = { id: "T1", title: "task", goal: "task", domain: "backend", depends_on: [], contracts: [], write_scope: ["a"], deliverables: [], acceptance: { commands: [], criteria: [] }, size: "S", difficulty: 1 };
beforeEach(async () => {
  await flushPipelines(); vi.restoreAllMocks(); resetScopeOwners();
  useExecution.setState({ pipelines: {}, closing: false });
  vi.spyOn(ipc, "savePipelineState").mockResolvedValue(undefined);
  vi.spyOn(ipc, "gitScopeSnapshot").mockResolvedValue(snapshot);
  vi.spyOn(ipc, "gitCaptureRollback").mockResolvedValue(undefined);
  vi.spyOn(ipc, "gitRollbackChanges").mockResolvedValue(["a", "new"]);
  vi.spyOn(ipc, "revertOutOfScopeFiles").mockResolvedValue("restored; recovery: /backup");
  useExecution.getState().startPlanExecution("s", [task], "/repo");
  useExecution.getState().claimTaskRunning("s", "T1", "turn");
  useExecution.getState().updateTaskState("s", "T1", "BLOCKED", "failed");
  useExecution.getState().recordTaskFailureEvidence("s", "T1", { workdir: "/repo", outputTail: "failure", outOfScopeFiles: ["new"], previousChangedFiles: ["a", "new"] });
});
describe("m1e rollback entrances", () => {
  it.each(["BLOCKED", "INTERRUPTED"] as const)("restores a %s turn with its exact identity", async (state) => {
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, taskStates: { T1: state } } } }));
    await restoreTaskChanges("s", "T1", true, () => true);
    expect(ipc.revertOutOfScopeFiles).toHaveBeenCalledWith("/repo", ["a", "new"], "turn");
    expect(useExecution.getState().pipelines.s.taskStates.T1).toBe(state);
    expect(useExecution.getState().pipelines.s.taskFailureEvidence?.T1.outOfScopeFiles).toEqual([]);
  });
  it("scope rollback selects only violations and keeps evidence after partial/native failure", async () => {
    vi.mocked(ipc.revertOutOfScopeFiles).mockRejectedValue(new Error("Rollback incomplete; recovery: /backup"));
    await expect(restoreTaskChanges("s", "T1", false, () => true)).rejects.toThrow("incomplete");
    expect(ipc.revertOutOfScopeFiles).toHaveBeenCalledWith("/repo", ["new"], "turn");
    expect(useExecution.getState().pipelines.s.taskFailureEvidence?.T1.outOfScopeFiles).toEqual(["new"]);
    expect(ipc.gitRollbackChanges).not.toHaveBeenCalled();
  });
  it("declining confirmation or missing sealed evidence performs no restore", async () => {
    expect(await restoreTaskChanges("s", "T1", true, () => false)).toBeNull();
    vi.mocked(ipc.gitRollbackChanges).mockRejectedValue(new Error("Missing after.json"));
    await expect(restoreTaskChanges("s", "T1", true, () => true)).rejects.toThrow("after.json");
    expect(ipc.revertOutOfScopeFiles).not.toHaveBeenCalled();
  });
  it("rejects active tasks, accepted Git deliveries, and a turn changed during confirmation", async () => {
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, taskStates: { T1: "RUNNING" } } } }));
    await expect(restoreTaskChanges("s", "T1", true, () => true)).rejects.toThrow("active");
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, taskStates: { T1: "BLOCKED" }, gitDeliveries: { T1: {} as ipc.TaskGitDelivery } } } }));
    await expect(restoreTaskChanges("s", "T1", true, () => true)).rejects.toThrow("delivery");
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, gitDeliveries: {} } } }));
    await expect(restoreTaskChanges("s", "T1", true, () => {
      useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, taskTurnIds: { T1: "next-turn" } } } })); return true;
    })).rejects.toThrow("changed");
    expect(ipc.revertOutOfScopeFiles).not.toHaveBeenCalled();
  });
  it("a failed content backup stops prelaunch and a rollback holds the checkout against dispatch", async () => {
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, taskStates: { T1: "RUNNING" } } } }));
    vi.mocked(ipc.gitCaptureRollback).mockRejectedValue(new Error("disk full"));
    await expect(prepareScopeBaseline("s", "T1", "turn", "/repo")).rejects.toThrow("disk full");
    expect(useExecution.getState().pipelines.s.scopeBaselines?.T1).toBeUndefined();
    releaseScopeOwner("turn");
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, taskStates: { T1: "BLOCKED" } } } }));
    vi.mocked(ipc.revertOutOfScopeFiles).mockImplementation(async () => {
      await expect(prepareScopeBaseline("s", "T1", "other", "/repo")).rejects.toThrow("owns");
      return "restored";
    });
    await restoreTaskChanges("s", "T1", true, () => true);
  });
});

it("keeps earlier-turn violations when restoring only the latest cancelled repair", async () => {
  useExecution.getState().recordTaskFailureEvidence("s", "T1", { turnId: "earlier", workdir: "/repo", outputTail: "prior violation", outOfScopeFiles: ["outside"], previousChangedFiles: ["outside"] });
  await restoreTaskChanges("s", "T1", true, () => true);
  expect(useExecution.getState().pipelines.s.taskFailureEvidence?.T1.outOfScopeFiles).toEqual(["outside"]);
  await restoreTaskChanges("s", "T1", false, () => true);
  expect(ipc.revertOutOfScopeFiles).toHaveBeenLastCalledWith("/repo", ["outside"], "earlier");
});

it("persists the failure turn identity across a cold pipeline reload", async () => {
  const { checkpointPipeline } = await import("../stores/execution");
  await checkpointPipeline("s");
  const saved = vi.mocked(ipc.savePipelineState).mock.lastCall![1];
  expect(saved.taskFailureEvidence?.T1.turnId).toBe("turn");
  vi.spyOn(ipc, "loadPipelineState").mockResolvedValue(saved);
  useExecution.setState({ pipelines: {} });
  await useExecution.getState().loadPipelineFromDisk("/repo", "s");
  expect(useExecution.getState().pipelines.s.taskFailureEvidence?.T1.turnId).toBe("turn");
  await restoreTaskChanges("s", "T1", false, () => true);
  expect(ipc.revertOutOfScopeFiles).toHaveBeenCalledWith("/repo", ["new"], "turn");
});
