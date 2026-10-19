import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as ipc from "./ipc";
import { runAcceptanceCommands } from "./acceptance";
import { cancelPlanAcceptance, deliverMergeToCheckout, maybeStartPlanDelivery, rerunPlanAcceptance, runGatesForTask } from "./taskRunner";
import { flushPipelines, useExecution } from "../stores/execution";

const task: ipc.TaskItem = { id: "T1", title: "one", goal: "one", domain: "backend", depends_on: [], contracts: [],
  write_scope: [], deliverables: [], acceptance: { commands: ["check-a", "check-b", "check-c"], criteria: [] }, size: "S", difficulty: 1 };
const snapshot = (): ipc.ScopeSnapshot => ({ root: "/integration", workdir: "/integration", prefix: "", clean: true, head: "accepted", files: {} });
const ok = (command: string): ipc.GateCommandResult => ({ command, passed: true, exitCode: 0, durationMs: 1, outputTail: "ok" });
const id = "acceptance-m1f";
function setup(commands = task.acceptance.commands, skipped = false) {
  const store = useExecution.getState();
  store.startPlanExecution(id, [{ ...task, acceptance: { ...task.acceptance, commands } }], "/project");
  store.setIntegration(id, { root: "/integration", branch: "tokage/integration", baseCommit: "base" });
  if (skipped) {
    store.updateTaskState(id, task.id, "SKIPPED");
    useExecution.setState(s => ({ pipelines: { ...s.pipelines, [id]: { ...s.pipelines[id], status: "DONE" } } }));
  }
  else {
    store.updateTaskState(id, task.id, "RUNNING");
    store.updateTaskState(id, task.id, "VERIFYING");
    store.updateTaskState(id, task.id, "DONE");
  }
}
beforeEach(async () => {
  await flushPipelines(); vi.restoreAllMocks();
  useExecution.setState({ pipelines: {}, closing: false });
  vi.spyOn(ipc, "savePipelineState").mockResolvedValue(undefined);
  vi.spyOn(ipc, "gitScopeSnapshot").mockImplementation(async () => snapshot());
  vi.spyOn(ipc, "gitRevParse").mockResolvedValue("accepted");
  vi.spyOn(ipc, "executeGateCommand").mockImplementation(async command => ok(command));
});
afterEach(async () => { await flushPipelines(); });

describe("m1f acceptance and normal delivery", () => {
  it.each(["failure", "throw", "cancel"])("retains all outcomes after %s and never reports aggregate success", async kind => {
    let active = true;
    const execute = vi.fn(async (command: string) => {
      if (command === "b") {
        if (kind === "throw") throw new Error("spawn failed");
        if (kind === "cancel") active = false;
        return { ...ok(command), passed: false, exitCode: 1 };
      }
      return ok(command);
    });
    const result = await runAcceptanceCommands(["a", "b", "c"], "T1", execute, () => active, () => {});
    expect(result.passed).toBe(false);
    expect(result.results.map(r => r.status)).toEqual(["passed", kind === "throw" ? "error" : kind === "cancel" ? "cancelled" : "failed", kind === "cancel" ? "cancelled" : "not_run"]);
    expect(execute).toHaveBeenCalledTimes(2);
  });

  it("empty task and plan commands finish execution without claiming automatic acceptance", async () => {
    setup([]);
    const gates = await runGatesForTask({ sessionId: id, taskId: task.id, task: { ...task, acceptance: { commands: [], criteria: [] } }, workdir: "/integration" });
    expect(gates.passed).toBe(false);
    await maybeStartPlanDelivery(id);
    expect(useExecution.getState().pipelines[id].delivery).toMatchObject({ acceptance: "not_configured", fullGatePassed: false, summary: { allGatesPassed: false } });
    const merge = vi.spyOn(ipc, "mergeIntegrationToCheckout");
    expect(await deliverMergeToCheckout(id)).toBe(false);
    expect(merge).not.toHaveBeenCalled();
  });

  it.each(["failure", "throw", "cancel"])("production full gate %s blocks normal merge and summary success", async kind => {
    setup();
    vi.mocked(ipc.executeGateCommand).mockImplementation(async command => {
      if (command === "check-b") {
        if (kind === "throw") throw new Error("gate unavailable");
        if (kind === "cancel") useExecution.setState(s => ({ pipelines: { ...s.pipelines, [id]: { ...s.pipelines[id], status: "INTERRUPTED" } } }));
        return { ...ok(command), passed: false, exitCode: 1 };
      }
      return ok(command);
    });
    await maybeStartPlanDelivery(id);
    const delivery = useExecution.getState().pipelines[id].delivery!;
    expect(delivery.fullGatePassed).toBe(false);
    expect(delivery.gateResults).toHaveLength(3);
    if (kind !== "cancel") expect(delivery.summary?.allGatesPassed).toBe(false);
    const merge = vi.spyOn(ipc, "mergeIntegrationToCheckout");
    expect(await deliverMergeToCheckout(id)).toBe(false);
    expect(merge).not.toHaveBeenCalled();
  });

  it("skipped tasks are excluded from completed reports and cannot authorize merge", async () => {
    setup(undefined, true);
    await maybeStartPlanDelivery(id);
    expect(useExecution.getState().pipelines[id].delivery).toMatchObject({ fullGatePassed: false, summary: { completedTasks: 0, skippedTasks: 1, allGatesPassed: false } });
    expect(await deliverMergeToCheckout(id)).toBe(false);
  });

  it.each(["disk", "index", "head", "commands"])("invalidates accepted evidence after %s change; explicit retry proves new content", async change => {
    setup(); await maybeStartPlanDelivery(id);
    expect(useExecution.getState().pipelines[id].delivery?.acceptance).toBe("passed");
    const changed = snapshot();
    if (change === "disk") changed.files["a"] = { disk: "new", index: "old" };
    if (change === "index") changed.files["a"] = { disk: "old", index: "new" };
    if (change === "head") changed.head = "new";
    if (change === "commands") useExecution.setState(s => ({ pipelines: { ...s.pipelines, [id]: { ...s.pipelines[id], tasks: [{ ...task, acceptance: { commands: ["new-check"], criteria: [] } }] } } }));
    vi.mocked(ipc.gitScopeSnapshot).mockResolvedValue(changed);
    vi.mocked(ipc.gitRevParse).mockResolvedValue(changed.head);
    const merge = vi.spyOn(ipc, "mergeIntegrationToCheckout").mockResolvedValue(undefined);
    expect(await deliverMergeToCheckout(id)).toBe(false);
    expect(merge).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines[id].delivery?.acceptance).toBe("stale");
    await rerunPlanAcceptance(id);
    // Dirty snapshots are not deliverable even if commands pass.
    expect(await deliverMergeToCheckout(id)).toBe(true);
    expect(merge).toHaveBeenCalledWith("/project", "tokage/integration", changed, undefined);
    expect(useExecution.getState().pipelines[id].delivery?.acceptedAt).toEqual(expect.any(Number));
  });

  it("command-side edits invalidate the run, including task gates", async () => {
    setup();
    vi.mocked(ipc.gitScopeSnapshot).mockResolvedValueOnce(snapshot()).mockResolvedValue({ ...snapshot(), files: { a: { disk: "new", index: "old" } } });
    await maybeStartPlanDelivery(id);
    expect(useExecution.getState().pipelines[id].delivery?.acceptance).toBe("stale");
    vi.mocked(ipc.gitScopeSnapshot).mockResolvedValueOnce(snapshot()).mockResolvedValue({ ...snapshot(), head: "other" });
    useExecution.setState(s => ({ pipelines: { ...s.pipelines, [id]: { ...s.pipelines[id], taskStates: { T1: "VERIFYING" } } } }));
    expect((await runGatesForTask({ sessionId: id, taskId: "T1", task, workdir: "/integration" })).passed).toBe(false);
  });

  it("explicit cancellation stops the production gate and rejects its late successful result", async () => {
    setup();
    let resolve!: (value: ipc.GateCommandResult) => void;
    const started = new Promise<void>(done => {
      vi.mocked(ipc.executeGateCommand).mockImplementationOnce(() => {
        done(); return new Promise<ipc.GateCommandResult>(r => { resolve = r; });
      });
    });
    const cancel = vi.spyOn(ipc, "cancelGateCommand").mockResolvedValue(true);
    const run = maybeStartPlanDelivery(id);
    await started;
    await cancelPlanAcceptance(id);
    expect(cancel).toHaveBeenCalledWith(`gate:delivery:${id}`);
    resolve(ok("check-a"));
    await run;
    expect(useExecution.getState().pipelines[id].delivery).toMatchObject({ acceptance: "cancelled", fullGatePassed: false });
    expect(await deliverMergeToCheckout(id)).toBe(false);
  });

  it("unreadable task snapshot records error and unrun commands without executing or claiming success", async () => {
    setup();
    useExecution.setState(s => ({ pipelines: { ...s.pipelines, [id]: { ...s.pipelines[id], taskStates: { T1: "VERIFYING" } } } }));
    vi.mocked(ipc.gitScopeSnapshot).mockRejectedValue(new Error("unreadable file"));
    const result = await runGatesForTask({ sessionId: id, taskId: "T1", task, workdir: "/integration" });
    expect(result.passed).toBe(false);
    expect(result.results.map(r => r.status)).toEqual(["error", "not_run", "not_run"]);
    expect(ipc.executeGateCommand).not.toHaveBeenCalled();
  });

  it("blocks legacy, running and failed evidence even when fullGatePassed is forged true", async () => {
    setup();
    const merge = vi.spyOn(ipc, "mergeIntegrationToCheckout");
    for (const phase of ["ready", "running_gates", "failed"] as const) {
      useExecution.getState().setDelivery(id, { phase, fullGatePassed: true });
      expect(await deliverMergeToCheckout(id)).toBe(false);
    }
    expect(merge).not.toHaveBeenCalled();
  });
});

describe("m2c delivery modes", () => {
  it("runs full gates inside the project subdirectory", async () => {
    setup();
    useExecution.getState().setIntegration(id, { root: "/integration", cwd: "/integration/packages/app", branch: "tokage/integration", baseCommit: "base" });
    await maybeStartPlanDelivery(id);
    expect(ipc.gitScopeSnapshot).toHaveBeenCalledWith("/integration/packages/app");
    for (const call of vi.mocked(ipc.executeGateCommand).mock.calls) expect(call[1]).toBe("/integration/packages/app");
  });

  it("exports pinned accepted commits and persists a recoverable source before keeping the branch", async () => {
    setup(); await maybeStartPlanDelivery(id);
    const exportPatch = vi.spyOn(ipc, "exportIntegrationPatch").mockResolvedValue(undefined);
    const remove = vi.spyOn(ipc, "removeTaskWorktree").mockResolvedValue(undefined);
    const { deliverExportPatch, deliverKeepBranch } = await import("./taskRunner");
    const path = await deliverExportPatch(id);
    expect(path).toMatch(/\.patch$/);
    expect(exportPatch).toHaveBeenCalledWith("/project", "base", "accepted", path);
    const source = { branch: "tokage/integration", commit: "accepted", baseCommit: "base" };
    expect(useExecution.getState().pipelines[id].delivery).toMatchObject({ phase: "exported", source, patchPath: path });
    await deliverKeepBranch(id);
    expect(remove).toHaveBeenCalledWith("/integration", "tokage/integration", true, false, "accepted");
    expect(useExecution.getState().pipelines[id].delivery).toMatchObject({ phase: "kept", source, patchPath: path });
    expect(useExecution.getState().pipelines[id].integration).toBeNull();
    expect(vi.mocked(ipc.savePipelineState).mock.calls.some(([, saved]) => saved.integration != null && saved.delivery?.source?.commit === "accepted")).toBe(true);
  });

  it.each(["dirty", "changed", "missing-base", "unaccepted"])("blocks both export and cleanup for %s evidence", async kind => {
    setup(); await maybeStartPlanDelivery(id);
    if (kind === "dirty") vi.mocked(ipc.gitScopeSnapshot).mockResolvedValue({ ...snapshot(), clean: false });
    if (kind === "changed") vi.mocked(ipc.gitRevParse).mockResolvedValue("other-commit");
    if (kind === "missing-base") useExecution.getState().setIntegration(id, { root: "/integration", branch: "tokage/integration" });
    if (kind === "unaccepted") useExecution.getState().setDelivery(id, { phase: "ready", fullGatePassed: true });
    const exportPatch = vi.spyOn(ipc, "exportIntegrationPatch");
    const remove = vi.spyOn(ipc, "removeTaskWorktree");
    const { deliverExportPatch, deliverKeepBranch } = await import("./taskRunner");
    expect(await deliverExportPatch(id)).toBeNull();
    await deliverKeepBranch(id);
    expect(exportPatch).not.toHaveBeenCalled(); expect(remove).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines[id].integration).not.toBeNull();
    expect(useExecution.getState().pipelines[id].delivery?.error).toBeTruthy();
  });

  it("preserves checkout metadata when nonforced cleanup fails, then permits retry", async () => {
    setup(); await maybeStartPlanDelivery(id);
    const remove = vi.spyOn(ipc, "removeTaskWorktree").mockRejectedValueOnce(new Error("DIRTY: human edited during delivery")).mockResolvedValue(undefined);
    const { deliverKeepBranch } = await import("./taskRunner");
    await deliverKeepBranch(id);
    expect(useExecution.getState().pipelines[id].integration?.root).toBe("/integration");
    expect(useExecution.getState().pipelines[id].delivery?.phase).toBe("ready");
    await deliverKeepBranch(id);
    expect(remove).toHaveBeenCalledTimes(2);
    expect(useExecution.getState().pipelines[id].delivery?.phase).toBe("kept");
  });

  it("serializes overlapping export and keep requests", async () => {
    setup(); await maybeStartPlanDelivery(id);
    let finish!: () => void;
    const exportPatch = vi.spyOn(ipc, "exportIntegrationPatch").mockImplementation(() => new Promise<void>(r => { finish = r; }));
    const remove = vi.spyOn(ipc, "removeTaskWorktree");
    const { deliverExportPatch, deliverKeepBranch } = await import("./taskRunner");
    const exporting = deliverExportPatch(id);
    await vi.waitFor(() => expect(exportPatch).toHaveBeenCalled());
    await deliverKeepBranch(id);
    expect(remove).not.toHaveBeenCalled();
    finish(); expect(await exporting).toMatch(/\.patch$/);
  });
});

describe("m2c latest persisted requirements", () => {
  async function bindPlan() {
    const { planIdentity } = await import("./planIdentity");
    setup();
    const p = useExecution.getState().pipelines[id];
    let current: ipc.Plan = { version: 1, tasks: p.tasks, contracts: [], spec: { raw: "", goal: "original", inScope: [], outOfScope: [], decisions: [] }, rawMarkdown: "" };
    useExecution.setState(s => ({ pipelines: { ...s.pipelines, [id]: { ...s.pipelines[id], planDefinition: planIdentity(current) } } }));
    vi.spyOn(ipc, "loadPlan").mockImplementation(async () => structuredClone(current));
    return (plan: ipc.Plan) => { current = plan; };
  }
  it("blocks all three deliveries after a saved requirement edit", async () => {
    const edit = await bindPlan();
    await maybeStartPlanDelivery(id);
    expect(useExecution.getState().pipelines[id].delivery?.acceptance).toBe("passed");
    edit({ version: 2, tasks: useExecution.getState().pipelines[id].tasks, contracts: [], spec: { raw: "", goal: "latest requirement", inScope: [], outOfScope: [], decisions: [] }, rawMarkdown: "" });
    const merge = vi.spyOn(ipc, "mergeIntegrationToCheckout");
    const exportPatch = vi.spyOn(ipc, "exportIntegrationPatch");
    const remove = vi.spyOn(ipc, "removeTaskWorktree");
    const { deliverExportPatch, deliverKeepBranch } = await import("./taskRunner");
    expect(await deliverMergeToCheckout(id)).toBe(false);
    expect(await deliverExportPatch(id)).toBeNull();
    await deliverKeepBranch(id);
    expect(merge).not.toHaveBeenCalled(); expect(exportPatch).not.toHaveBeenCalled(); expect(remove).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines[id].delivery?.acceptance).toBe("stale");
  });
  it("cannot relabel old execution as new acceptance when the edit predates the full gate", async () => {
    const edit = await bindPlan();
    edit({ version: 1, tasks: useExecution.getState().pipelines[id].tasks, contracts: [], spec: { raw: "", goal: "changed goal", inScope: [], outOfScope: [], decisions: [] }, rawMarkdown: "" });
    await maybeStartPlanDelivery(id);
    expect(ipc.executeGateCommand).not.toHaveBeenCalled();
    expect(useExecution.getState().pipelines[id].delivery?.fullGatePassed).toBe(false);
  });
});
