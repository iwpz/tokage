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
    expect(merge).toHaveBeenCalledWith("/project", "tokage/integration", changed);
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
