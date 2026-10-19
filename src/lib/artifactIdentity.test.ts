import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import * as ipc from "./ipc";
import { checkpointPipeline, flushPipelines, useExecution } from "../stores/execution";
import { useProjects } from "../stores/projects";
import { canReplan, confirmReplan } from "./replan";
import { maybeStartPlanDelivery } from "./taskRunner";

const task: ipc.TaskItem = { id: "T1", title: "one", goal: "one", domain: "backend",
  depends_on: [], contracts: [], write_scope: [], deliverables: [],
  acceptance: { commands: [], criteria: [] }, size: "S", difficulty: 1 };
const plan: ipc.Plan = { version: 1, tasks: [task, { ...task, id: "T2" }], contracts: [],
  spec: { raw: "", goal: "goal", inScope: [], outOfScope: [], decisions: [] }, rawMarkdown: "" };
const report = (sessionId: string, planRecordId: number, taskId = "T1"): ipc.TaskReportRow => ({
  sessionId, planRecordId, taskId, modelId: "model", attemptId: `${sessionId}-attempt`, firstPass: true,
  isLocallyGenerated: false, summaryMd: `${sessionId} summary`, fullMd: `${sessionId} full`,
  changedFilesStat: "1 file", changedFilesList: [`${sessionId}.ts`], gateResults: "ok", costUsd: 0.75,
});

beforeEach(async () => {
  await flushPipelines();
  vi.restoreAllMocks();
  useExecution.setState({ pipelines: {}, closing: false });
  useProjects.setState({ projects: [{ id: "p", name: "project", path: "/test", expanded: true,
    sessions: ["s1", "s2"].map((id) => ({ id, title: id, state: "running", updatedAt: 1 })) }] });
  vi.spyOn(ipc, "savePipelineState").mockResolvedValue(undefined);
});
afterEach(async () => { await flushPipelines(); });

function pipeline(sessionId: string, planId: number) {
  const store = useExecution.getState();
  store.startPlanExecution(sessionId, plan.tasks, "/test");
  store.setPlanDbIds(sessionId, planId, { T1: planId * 10, T2: planId * 10 + 1 });
  for (const t of plan.tasks) {
    store.updateTaskState(sessionId, t.id, "RUNNING");
    store.updateTaskState(sessionId, t.id, "VERIFYING");
    store.updateTaskState(sessionId, t.id, "DONE");
  }
}

describe("m1b artifact identity consumers", () => {
  it("restores task mappings and explicitly inherited report origins, never queries a local code alone", async () => {
    vi.spyOn(ipc, "loadPipelineState").mockImplementation(async (_cwd, sessionId) => ({
      schemaVersion: 2, sessionId, status: "DONE", planDbId: sessionId === "s1" ? 11 : 21,
      taskDbIds: { T1: 101 }, taskReportPlanIds: { T1: sessionId === "s1" ? 10 : 20 },
      taskItems: [task], tasks: [{ id: "T1", title: "one", state: "DONE" }],
      taskStates: { T1: "DONE" }, taskLogs: {}, gateResults: [], startedAt: 1,
    }));
    const read = vi.spyOn(ipc, "getTaskReports").mockImplementation(async (sid, pid) => [report(sid, pid)]);
    const first = await useExecution.getState().loadPipelineFromDisk("/test", "s1");
    const second = await useExecution.getState().loadPipelineFromDisk("/test", "s2");
    expect(read.mock.calls).toEqual([["s1", 10, "T1"], ["s2", 20, "T1"]]);
    expect(first?.upstreamArtifacts?.T1.changedFiles).toEqual(["s1.ts"]);
    expect(second?.upstreamArtifacts?.T1.report).toBe("s2 summary");
    expect(first?.taskDbIds).toEqual({ T1: 101 });
    await checkpointPipeline("s1");
    expect(vi.mocked(ipc.savePipelineState).mock.lastCall?.[1]).toMatchObject({ taskDbIds: { T1: 101 }, taskReportPlanIds: { T1: 10 } });
  });

  it("does not auto-associate historical pipelines without a plan identity", async () => {
    vi.spyOn(ipc, "loadPipelineState").mockResolvedValue({ schemaVersion: 2, sessionId: "s1", status: "DONE",
      taskItems: [task], tasks: [{ id: "T1", title: "one", state: "DONE" }],
      taskStates: { T1: "DONE" }, taskLogs: {}, gateResults: [], startedAt: 1 });
    const read = vi.spyOn(ipc, "getTaskReports");
    const restored = await useExecution.getState().loadPipelineFromDisk("/test", "s1");
    expect(read).not.toHaveBeenCalled();
    expect(restored?.upstreamArtifacts).toEqual({});
  });

  it("builds delivery files and costs from each session's exact report scope", async () => {
    pipeline("s1", 11); pipeline("s2", 21);
    vi.spyOn(ipc, "loadPlan").mockResolvedValue(plan);
    const read = vi.spyOn(ipc, "getTaskReports").mockImplementation(async (sid, pid, taskId) => [report(sid, pid, taskId)]);
    await Promise.all([maybeStartPlanDelivery("s1"), maybeStartPlanDelivery("s2")]);
    expect(read).toHaveBeenCalledWith("s1", 11, "T1");
    expect(read).toHaveBeenCalledWith("s2", 21, "T1");
    const sessions = useExecution.getState().pipelines;
    expect(sessions.s1.delivery?.summary).toMatchObject({ touchedFiles: ["s1.ts", "s1.ts"], actualCostUsd: 1.5 });
    expect(sessions.s2.delivery?.summary).toMatchObject({ touchedFiles: ["s2.ts", "s2.ts"], actualCostUsd: 1.5 });
  });

  it.each([false, true])("replan invalidates changed evidence and dependent=%s report origins", async dependent => {
    const current = { ...plan, tasks: [task, { ...plan.tasks[1], depends_on: dependent ? ["T1"] : [] }] };
    pipeline("s1", 11);
    useExecution.setState((s) => ({ pipelines: { ...s.pipelines, s1: { ...s.pipelines.s1,
      taskWorktrees: { T1: { root: "/preserved/T1", cwd: "/preserved/T1/app", branch: "tokage/old-T1", baseCommit: "base" } },
      threadIds: { T1: "old1", T2: "old2" }, taskTurnIds: { T1: "attempt1", T2: "attempt2" },
      upstreamArtifacts: { T1: { taskId: "T1", title: "one", report: "old", changedFiles: ["old.ts"] } },
      delivery: { phase: "ready" }, taskReportPlanIds: { T2: 7 },
    } } }));
    useExecution.setState(s => ({ pipelines: { ...s.pipelines, s1: { ...s.pipelines.s1, tasks: current.tasks } } }));
    const next: ipc.Plan = { ...current, version: 2, tasks: [{ ...task, goal: "new goal" }, current.tasks[1]] };
    vi.spyOn(ipc, "loadPlan").mockResolvedValue(current);
    vi.spyOn(ipc, "applyPlanDiff").mockResolvedValue(next);
    vi.spyOn(ipc, "persistPlan").mockResolvedValue({ planId: "2", planDir: "/test", filesWritten: [] });
    const read = vi.spyOn(ipc, "getTaskReports").mockImplementation(async (sid, pid, tid) => [report(sid, pid, tid)]);
    const adaptation = vi.spyOn(ipc, "generateAdaptationTasks").mockResolvedValue([]);
    vi.spyOn(ipc, "recordPlanApproved").mockResolvedValue({ planId: 12, taskIds: { T1: 120, T2: 121 } });
    const diff = await ipc.computePlanDiff(plan, next);
    // Specify the actual identity change independently of the browser diff implementation.
    diff.tasks = [ { id: "T1", title: "one", action: "modified", contractsChanged: false, dependenciesChanged: false, scopeChanged: false },
      { id: "T2", title: "two", action: "unchanged", contractsChanged: false, dependenciesChanged: false, scopeChanged: false } ];
    const result = await confirmReplan({ sessionId: "s1", currentPlan: current, proposed: next, diff });
    expect("error" in result).toBe(false);
    expect(read.mock.calls).toEqual([["s1", 11, "T1"], ["s1", 7, "T2"]]);
    expect(adaptation.mock.calls[0][0].taskReports[0]).toMatchObject({ fullReport: "s1 full", modifiedFiles: ["s1.ts"] });
    const updated = useExecution.getState().pipelines.s1;
    expect(updated).toMatchObject({ planDbId: 12, taskDbIds: { T1: 120, T2: 121 },
      taskStates: { T1: "PENDING", T2: dependent ? "PENDING" : "DONE" }, taskReportPlanIds: dependent ? {} : { T2: 7 }, status: "PAUSED", delivery: null });
    expect(updated.threadIds?.T1).toBeUndefined();
    expect(updated.taskTurnIds?.T1).toBeUndefined();
    expect(updated.taskLogs._replan.join("\n")).toContain("/preserved/T1");
    expect(updated.taskWorktrees?.T1).toBeUndefined();
    expect(updated.upstreamArtifacts?.T1).toBeUndefined();
    expect(updated.threadIds?.T2).toBe(dependent ? undefined : "old2");
    if (dependent) expect(updated.taskReportPlanIds?.T2).toBeUndefined();
  });

  it("rejects a stale saved plan without mutating or persisting the proposal", async () => {
    pipeline("s1", 11);
    vi.spyOn(ipc, "loadPlan").mockResolvedValue({ ...plan, version: 2 });
    const persist = vi.spyOn(ipc, "persistPlan");
    const result = await confirmReplan({ sessionId: "s1", currentPlan: plan, proposed: plan, diff: await ipc.computePlanDiff(plan, plan) });
    expect("error" in result).toBe(true);
    expect(persist).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines.s1.planDbId).toBe(11);
  });

  it.each(["DONE", "BLOCKED"] as const)("refuses deleting integrated work with state %s", async (state) => {
    pipeline("s1", 11);
    useExecution.getState().setIntegration("s1", { root: "/integration", branch: "tokage/test", baseCommit: "base" });
    useExecution.setState(s => ({ pipelines: { ...s.pipelines, s1: { ...s.pipelines.s1, taskStates: { ...s.pipelines.s1.taskStates, T1: state }, gitDeliveries: { T1: { phase: "merged" } as ipc.TaskGitDelivery } } } }));
    vi.spyOn(ipc, "loadPlan").mockResolvedValue(plan);
    const persist = vi.spyOn(ipc, "persistPlan");
    const proposed = { ...plan, tasks: [plan.tasks[1]] };
    const result = await confirmReplan({ sessionId: "s1", currentPlan: plan, proposed, diff: await ipc.computePlanDiff(plan, proposed) });
    expect("error" in result && result.error).toContain("explicit undo task");
    expect(persist).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines.s1.integration?.root).toBe("/integration");
  });

  it("prevents replan while a paused pipeline still has an active attempt", () => {
    pipeline("s1", 11);
    useExecution.setState((s) => ({ pipelines: { s1: { ...s.pipelines.s1, status: "PAUSED", taskStates: { T1: "VERIFYING" } } } }));
    expect(canReplan("s1")).toBe("pipelineBusy");
  });
});
