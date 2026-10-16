import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import * as ipc from "./ipc";
import { checkTaskScope, prepareScopeBaseline, releaseScopeOwner, pinScopeOwner, resetScopeOwners } from "./scopeGuard";
import { flushPipelines, useExecution } from "../stores/execution";
import { useSettings } from "../stores/settings";
import { processSubAgentCompletion, resetSubAgentEventListener, completeTakeoverAndRerunGates } from "./taskRunner";

const task: ipc.TaskItem = { id: "T1", title: "scope", goal: "scope", domain: "backend", depends_on: [],
  contracts: [], write_scope: ["src/a", "src/new"], deliverables: [],
  acceptance: { commands: ["test"], criteria: [] }, size: "S", difficulty: 1 };
const next = { ...task, id: "T2", depends_on: ["T1"] };
const file = (disk: string, index = "old-index") => ({ disk, index });
const baseline: ipc.ScopeSnapshot = { root: "/repo", workdir: "/repo", prefix: "", head: "base", clean: true,
  files: { "src/a": file("before"), "user-dirty": file("user-content") } };
let disk: ipc.ScopeSnapshot;
const ctx = () => ({ sessionId: "s", taskId: "T1", task, worktreeCwd: "/repo", turnId: "turn",
  rawOutput: "done", changedFiles: [] as string[], toolCalls: [], startTime: 1 });

beforeEach(async () => {
  await flushPipelines();
  vi.restoreAllMocks(); resetSubAgentEventListener(); resetScopeOwners();
  useExecution.setState({ pipelines: {}, closing: false });
  useSettings.setState({ isolation: "inplace" });
  vi.spyOn(ipc, "savePipelineState").mockResolvedValue(undefined);
  disk = structuredClone(baseline);
  vi.spyOn(ipc, "gitScopeSnapshot").mockImplementation(async () => structuredClone(disk));
  vi.spyOn(ipc, "validateWriteScope").mockImplementation(async (scopes, paths) => ({
    passed: paths.every((p) => scopes.includes(p)), declaredScopes: scopes, changedFiles: paths,
    allowedFiles: paths.filter((p) => scopes.includes(p)), outOfScopeFiles: paths.filter((p) => !scopes.includes(p)),
  }));
  vi.spyOn(ipc, "executeGateCommand").mockResolvedValue({ command: "test", passed: true, durationMs: 1, outputTail: "ok" });
  vi.spyOn(ipc, "recordTaskReport").mockResolvedValue(1);
  vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);
  vi.spyOn(ipc, "commitTaskWorktree").mockResolvedValue(true);
  vi.spyOn(ipc, "mergeTaskWorktree");
  useExecution.getState().startPlanExecution("s", [task, next], "/repo");
  useExecution.getState().claimTaskRunning("s", "T1", "turn");
  useExecution.setState((s) => ({ pipelines: { ...s.pipelines, s: { ...s.pipelines.s,
    scopeBaselines: { T1: structuredClone(baseline) }, planDbId: 1,
  } } }));
});
afterEach(async () => { await flushPipelines(); resetScopeOwners(); });
function expectBlocked() {
  const p = useExecution.getState().pipelines.s;
  expect(p.taskStates.T1).toBe("BLOCKED"); expect(p.taskStates.T2).toBe("PENDING");
  expect(ipc.commitTaskWorktree).not.toHaveBeenCalled(); expect(ipc.mergeTaskWorktree).not.toHaveBeenCalled();
  expect(ipc.startChatTurn).not.toHaveBeenCalled();
}

describe("m1c scope guard", () => {
  it.each(["missing baseline", "Git failed", "scope IPC failed", "HEAD changed"])("blocks completion when %s", async (reason) => {
    if (reason === "missing baseline") useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, scopeBaselines: {} } } }));
    if (reason === "Git failed") vi.mocked(ipc.gitScopeSnapshot).mockRejectedValue(new Error("Git read denied"));
    if (reason === "scope IPC failed") vi.mocked(ipc.validateWriteScope).mockRejectedValue(new Error("scope IPC unavailable"));
    if (reason === "HEAD changed") disk.head = "another-commit";
    await processSubAgentCompletion(ctx());
    expectBlocked(); expect(ipc.executeGateCommand).not.toHaveBeenCalled();
    expect(ipc.recordTaskReport).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines.s.taskFailureEvidence?.T1.outputTail).toBeTruthy();
  });

  it("detects CLI-omitted disk writes, including edits to an already dirty path", async () => {
    disk.files["user-dirty"] = file("agent overwrote user content");
    disk.files["outside-new"] = file("created", "");
    await processSubAgentCompletion(ctx());
    expectBlocked(); expect(ipc.executeGateCommand).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines.s.taskFailureEvidence?.T1.outOfScopeFiles).toEqual(["outside-new", "user-dirty"]);
  });

  it("compares both index and disk while excluding unchanged user dirt", async () => {
    disk.files["src/a"] = file("before", "new-staged-index");
    disk.files["src/new"] = file("new untracked", "");
    const evidence = await checkTaskScope("s", "T1", "turn", "/repo", task.write_scope);
    expect(evidence.changedFiles).toEqual(["src/a", "src/new"]);
    expect(useExecution.getState().pipelines.s.scopeChecks?.T1.snapshot).toEqual(disk);
    expect(vi.mocked(ipc.savePipelineState).mock.lastCall?.[1]).toMatchObject({ scopeBaselines: { T1: baseline }, scopeChecks: { T1: { snapshot: disk } } });
  });

  it("never excuses ambiguous parallel changes using another task's write scope", async () => {
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s,
      tasks: [task, { ...next, write_scope: ["outside"] }], taskStates: { T1: "RUNNING", T2: "RUNNING" },
      scopeBaselines: { T1: baseline, T2: baseline },
    } } }));
    disk.files.outside = file("other or mine", "");
    await processSubAgentCompletion(ctx());
    expect(useExecution.getState().pipelines.s.taskStates.T1).toBe("BLOCKED");
    expect(ipc.executeGateCommand).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines.s.taskFailureEvidence?.T1.outputTail).toContain("cannot be attributed");
  });

  it("post-gate out-of-scope output blocks report, commit and downstream", async () => {
    vi.mocked(ipc.executeGateCommand).mockImplementation(async () => {
      disk.files["outside-generated"] = file("gate generated", "");
      return { command: "test", passed: true, durationMs: 1, outputTail: "ok" };
    });
    await processSubAgentCompletion(ctx());
    expectBlocked(); expect(ipc.executeGateCommand).toHaveBeenCalledOnce();
    expect(ipc.recordTaskReport).not.toHaveBeenCalled();
  });

  it("failed gates cannot hide outside writes behind automatic repair", async () => {
    vi.mocked(ipc.executeGateCommand).mockImplementation(async () => {
      disk.files["outside-failed-test"] = file("generated before failure", "");
      return { command: "test", passed: false, durationMs: 1, outputTail: "failed" };
    });
    await processSubAgentCompletion(ctx());
    expectBlocked(); expect(ipc.recordTaskReport).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines.s.taskFailureEvidence?.T1.outOfScopeFiles).toContain("outside-failed-test");
  });

  it("final disk evidence overrides invented model file lists", async () => {
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, tasks: [task], taskStates: { T1: "RUNNING" } } } }));
    disk.files["src/a"] = file("actual write");
    vi.spyOn(ipc, "parseOrGenerateTaskReport").mockResolvedValue({ taskId: "T1", title: "scope", model: "m", attemptId: "turn",
      firstPass: true, isLocallyGenerated: false, completedWork: "done", tipsForDownstream: "none", selfCheckCriteria: [],
      changedFilesStat: "invented", changedFilesList: ["invented"], gateResultsSummary: "ok" });
    await processSubAgentCompletion(ctx());
    expect(vi.mocked(ipc.recordTaskReport).mock.calls[0][0].changedFilesList).toEqual(["src/a"]);
    expect(useExecution.getState().pipelines.s.upstreamArtifacts?.T1.changedFiles).toEqual(["src/a"]);
    expect(useExecution.getState().pipelines.s.taskStates.T1).toBe("DONE");
  });

  it("a post-gate scope IPC failure also blocks successful completion", async () => {
    vi.mocked(ipc.validateWriteScope).mockResolvedValueOnce({ passed: true, declaredScopes: task.write_scope,
      changedFiles: [], allowedFiles: [], outOfScopeFiles: [] }).mockRejectedValueOnce(new Error("post-gate IPC failed"));
    await processSubAgentCompletion(ctx());
    expectBlocked(); expect(ipc.executeGateCommand).toHaveBeenCalledOnce();
    expect(ipc.recordTaskReport).not.toHaveBeenCalled();
  });

  it("normalizes absolute in-directory CLI paths without treating events as disk evidence", async () => {
    const evidence = await checkTaskScope("s", "T1", "turn", "/repo", task.write_scope, ["/repo/src/a"]);
    expect(evidence.changedFiles).toEqual([]);
    expect(ipc.validateWriteScope).toHaveBeenCalledWith(task.write_scope, ["src/a"]);
  });

  it("worktree completion cannot bypass verified commit with missing metadata", async () => {
    useSettings.setState({ isolation: "worktree" });
    await processSubAgentCompletion(ctx());
    expectBlocked();
    expect(useExecution.getState().pipelines.s.taskLogs.T1.join(" ")).toContain("Missing worktree");
  });

  it("worktree takeover also requires the verified commit path", async () => {
    useSettings.setState({ isolation: "worktree" });
    useExecution.getState().updateTaskState("s", "T1", "BLOCKED");
    expect(await completeTakeoverAndRerunGates("s", "T1")).toBe(false);
    expectBlocked();
  });

  it("changing isolation settings cannot bypass a worktree baseline's commit requirement", async () => {
    useSettings.setState({ isolation: "inplace" });
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s,
      scopeBaselines: { T1: { ...baseline, executionMode: "worktree" } },
    } } }));
    await processSubAgentCompletion(ctx());
    expectBlocked();
  });

  it("files changing after report storage cannot reach DONE or commit", async () => {
    vi.mocked(ipc.recordTaskReport).mockImplementation(async () => { disk.files["src/a"] = file("late in-scope edit"); return 1; });
    await processSubAgentCompletion(ctx());
    expectBlocked();
    expect(useExecution.getState().pipelines.s.taskFailureEvidence?.T1.outputTail).toContain("after acceptance");
  });

  it("takeover also blocks on a missing original baseline", async () => {
    useExecution.getState().updateTaskState("s", "T1", "BLOCKED");
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, scopeBaselines: {} } } }));
    expect(await completeTakeoverAndRerunGates("s", "T1")).toBe(false);
    expectBlocked(); expect(ipc.executeGateCommand).not.toHaveBeenCalled();
  });

  it("cannot replace a missing resume baseline with the already edited tree", async () => {
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, scopeBaselines: {}, threadIds: { T1: "old-thread" } } } }));
    await expect(prepareScopeBaseline("s", "T1", "turn", "/repo")).rejects.toThrow("original scope baseline");
  });

  it("holds the checkout through a cancelled gate until that command settles", async () => {
    await checkTaskScope("s", "T1", "turn", "/repo", task.write_scope);
    const unpin = pinScopeOwner("turn"); releaseScopeOwner("turn");
    await expect(prepareScopeBaseline("s", "T2", "other", "/repo")).rejects.toThrow("owns this shared checkout");
    unpin();
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s,
      taskStates: { T1: "DONE", T2: "RUNNING" }, taskTurnIds: { T2: "other" },
    } } }));
    await expect(prepareScopeBaseline("s", "T2", "other", "/repo")).resolves.toBeUndefined();
  });

  it("rejects outside-subdirectory changes even with a broad declared scope", async () => {
    const sub = { ...baseline, workdir: "/repo/src", prefix: "src" };
    disk = { ...structuredClone(sub), files: { ...sub.files, outside: file("write", "") } };
    useExecution.setState((s) => ({ pipelines: { s: { ...s.pipelines.s, scopeBaselines: { T1: sub } } } }));
    await expect(checkTaskScope("s", "T1", "turn", "/repo/src", ["**"])).rejects.toThrow("outside the task directory");
  });
});
