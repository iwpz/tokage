import { beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: vi.fn() }));
import { useSettings } from "../stores/settings";
import { useProjects } from "../stores/projects";
import { useChat } from "../stores/chat";
import { useExecution } from "../stores/execution";
import { useProviders } from "../stores/providers";
import { DEFAULT_DEBUG_PROVIDER } from "./debugProviderTypes";
import { demoConfig, demoIdentity, DEMO_PROJECT_ID } from "./demoIdentity";
import { prepareDemo, hideDemo } from "./demoMode";
import { demoPlan, demoScript } from "./demoScript";
import { allocatePlanModels, loadPlan, persistPlan, onChatEvent, startChatTurn, cancelChatTurn, onChatEnded } from "./ipc";
import { currentAvailableModels } from "./models";
import { dispatchAsync } from "./dispatch";
import { dispatchReadyTasks, maybeStartPlanDelivery, resetSubAgentEventListener } from "./taskRunner";
import { useDemoAudit } from "./demoAudit";
import { planIdentity } from "./planIdentity";

beforeEach(() => {
  resetSubAgentEventListener();
  useSettings.setState({ language: "en", demoEnabled: false, demoScenario: "success", demoChunkDelayMs: 0, debugProvider: { ...DEFAULT_DEBUG_PROVIDER }, isolation: "inplace", providerEnabled: {}, providerTrust: {}, maxParallel: 3 });
  useProjects.setState({ projects: [], activeProjectId: null, activeSessionId: null });
  useChat.setState({ conversations: {} });
  useExecution.setState({ pipelines: {} });
  useProviders.setState({ providers: [], realProviders: [] });
  useDemoAudit.setState({ turns: {} });
});

describe("bilingual demo mode", () => {
  it.each(["en", "zh"] as const)("seeds a real reviewable seven-task %s plan without changing ordinary routing", async language => {
    useSettings.setState({ language });
    await prepareDemo();
    const project = useProjects.getState().projects[0];
    const session = project.sessions[0];
    expect(project).toMatchObject({ id: DEMO_PROJECT_ID, name: "demo_project", trusted: true });
    expect(session).toMatchObject({ title: "demo_work", state: "review" });
    const conversation = useChat.getState().conversations[session.id];
    expect(conversation.messages[0].text).toBe(demoScript(language).request);
    expect(conversation.messages[1]).toMatchObject({ modelName: "Fable 5.1", debugRoute: { actualProvider: "DEBUG_PROVIDER", actualModel: "mock" }, usage: { input: 0, output: 0 } });
    const plan = await loadPlan(project.path, session.id);
    expect(plan.tasks).toHaveLength(7);
    expect(plan.tasks[4].depends_on).toEqual(["T2", "T3"]);
    expect(useExecution.getState().pipelines[session.id]).toBeUndefined();
    expect(useSettings.getState().debugProvider.output).toBe("off");
    await prepareDemo();
    expect(useProjects.getState().projects).toHaveLength(1);
    expect(useProjects.getState().activeSessionId).toBe(session.id);
    hideDemo();
    expect(useSettings.getState().demoEnabled).toBe(false);
    expect(demoConfig(session.id)?.output).toBe("mock");
  });

  it("reset snapshots scenario and language into a new isolated run", async () => {
    await prepareDemo();
    const old = useProjects.getState().projects[0];
    useSettings.setState({ language: "zh", demoScenario: "repair" });
    await prepareDemo(true);
    const next = useProjects.getState().projects[0];
    expect(next.path).not.toBe(old.path);
    expect(demoIdentity(next.sessions[0].id)).toEqual({ language: "zh", scenario: "repair" });
    expect(useChat.getState().conversations[old.sessions[0].id]).toBeUndefined();
  });

  it("streams localized planning through Mock even when a paid debug source is configured", async () => {
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "luna" } });
    let text = "";
    const off = await onChatEvent(event => { if (event.turnId === "demo-plan" && event.kind === "text") text += event.text; });
    await startChatTurn({ turnId: "demo-plan", sessionId: "tokage-demo-success-zh-plan", cwd: "/demo", prompt: "plan", mode: "plan", provider: "CLAUDE_CODE", model: "fable", effort: null, fast: false, tier: null, resume: null, history: [], trusted: [] });
    await vi.waitFor(() => expect(text).toBe(demoPlan("zh").rawMarkdown), { timeout: 5000 });
    off();
  });

  it.each(["success", "repair"] as const)("runs the %s script through the task runner, dependencies and delivery with no real providers", async scenario => {
    useSettings.setState({ demoScenario: scenario });
    await prepareDemo();
    const project = useProjects.getState().projects[0];
    const session = project.sessions[0];
    const plan = await loadPlan(project.path, session.id);
    const allocation = await allocatePlanModels(plan, {}, undefined, currentAvailableModels(session.id));
    const tasks = plan.tasks.map(task => ({ ...task, assigned_model: "CLAUDE_CODE:sonnet" }));
    // Approval persists the allocated definition before execution, as PlanReview does.
    const approved = { ...plan, tasks };
    await persistPlan(project.path, session.id, approved);
    useExecution.getState().startPlanExecution(session.id, tasks, project.path, plan.contracts, allocation.assignments, planIdentity(approved));
    await dispatchReadyTasks(session.id);
    await vi.waitFor(() => {
      expect(useExecution.getState().pipelines[session.id].taskStates).toEqual(Object.fromEntries(tasks.map(task => [task.id, "DONE"])));
    }, { timeout: 10000 });
    await vi.waitFor(() => {
      expect(useExecution.getState().pipelines[session.id].delivery).toMatchObject({
        phase: "ready", acceptance: "passed", fullGatePassed: true, error: null,
      });
    }, { timeout: 5000 });
    const pipeline = useExecution.getState().pipelines[session.id];
    expect(pipeline.delivery?.evidence?.plan).toBe(planIdentity(approved));
    for (const task of tasks) for (const dependency of task.depends_on) {
      expect(pipeline.taskStartedAt![task.id]).toBeGreaterThanOrEqual(pipeline.taskEndedAt![dependency]);
    }
    const audit = Object.values(useDemoAudit.getState().turns);
    expect(audit.length).toBeGreaterThanOrEqual(7);
    expect(audit.every(turn => turn.route === "DEBUG_PROVIDER/mock" && turn.inputTokens === 0 && turn.outputTokens === 0 && turn.apiCalls === 0 && turn.costUsd === 0)).toBe(true);
    if (scenario === "repair") {
      const gates = pipeline.gateResults!.filter(gate => gate.taskId === "T2");
      expect(gates.map(gate => gate.passed)).toEqual([false, true]);
    }
  });
  it("rejects a demo plan edited after approval instead of bypassing the delivery gate", async () => {
    await prepareDemo();
    const project = useProjects.getState().projects[0];
    const session = project.sessions[0];
    const plan = await loadPlan(project.path, session.id);
    useExecution.getState().startPlanExecution(session.id, plan.tasks, project.path, plan.contracts, [], planIdentity(plan));
    useExecution.setState(state => ({ pipelines: { ...state.pipelines, [session.id]: {
      ...state.pipelines[session.id], status: "DONE",
      taskStates: Object.fromEntries(plan.tasks.map(task => [task.id, "DONE" as const])),
    } } }));
    const edited = structuredClone(plan);
    edited.tasks[0].goal = "Changed after approval";
    await persistPlan(project.path, session.id, edited);
    await maybeStartPlanDelivery(session.id);
    expect(useExecution.getState().pipelines[session.id].delivery).toMatchObject({
      phase: "failed", fullGatePassed: false,
      error: expect.stringContaining("apply the edited plan through replan"),
    });
  });

  it("a new conversation in the demo project retains its Mock identity and can plan without installed CLIs", async () => {
    await prepareDemo();
    const id = useProjects.getState().createSession(DEMO_PROJECT_ID, "Another demo");
    expect(demoIdentity(id)).toEqual({ scenario: "success", language: "en" });
    expect(await dispatchAsync(id, "Please plan the demo", "plan")).toBe("sent");
    await vi.waitFor(() => {
      const reply = useChat.getState().conversations[id].messages.at(-1)!;
      expect(reply.status).toBe("done");
      expect(reply.debugRoute?.actualProvider).toBe("DEBUG_PROVIDER");
      expect(reply.text).toBe(demoPlan("en").rawMarkdown);
    }, { timeout: 5000 });
  });

  it("slow demo cancellation ends once and emits no late success", async () => {
    const events: string[] = [];
    let ended = 0;
    const off = await onChatEvent(event => { if (event.turnId === "demo-slow") events.push(event.kind); });
    const offEnd = await onChatEnded(event => { if (event.turnId === "demo-slow") { ended++; expect(event.cancelled).toBe(true); } });
    await startChatTurn({ turnId: "demo-slow", sessionId: "tokage-demo-slow-en-cancel", taskCode: "T3", cwd: "/demo", prompt: "slow", mode: "direct", provider: "CLAUDE_CODE", model: "sonnet", effort: null, fast: false, tier: null, resume: null, history: [], trusted: [] });
    await cancelChatTurn("demo-slow");
    await new Promise(resolve => setTimeout(resolve, 20));
    expect(ended).toBe(1); expect(events).not.toContain("done");
    off(); offEnd();
  });

  it("execution errors are localized and target T3, while planning still works", async () => {
    const errors: string[] = [];
    const off = await onChatEvent(event => { if (event.turnId === "demo-error" && event.kind === "error") errors.push(event.message); });
    await startChatTurn({ turnId: "demo-error", sessionId: "tokage-demo-error-zh-failure", taskCode: "T3", cwd: "/demo", prompt: "fail", mode: "direct", provider: "CLAUDE_CODE", model: "sonnet", effort: null, fast: false, tier: null, resume: null, history: [], trusted: [] });
    await vi.waitFor(() => expect(errors).toEqual([demoScript("zh").failure]));
    off();
  });

});
