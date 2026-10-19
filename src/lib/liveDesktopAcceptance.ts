/** Explicit acceptance bundle only. Exercises the rendered desktop UI and real
 * Tauri IPC; no mocked turns, gates, Git operations or completion events. */
import { invoke } from "@tauri-apps/api/core";
import i18n from "../i18n";
import { useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";
import { useChat } from "../stores/chat";
import { useAuth } from "../stores/auth";
import { useUi } from "../stores/ui";
import { useExecution, flushPipelines } from "../stores/execution";
import { DEFAULT_DEBUG_PROVIDER } from "./debugProviderTypes";
import { ensureSubAgentEventListener, cancelTask, cancelPlanAcceptance, continueTask } from "./taskRunner";
import { onChatEvent, persistPlan, parsePlanningOutput, createSessionWorktree, type Plan, type ProviderId, type TaskWorktreeInfo, taskRecoveryEvidence } from "./ipc";
import { reviewNaturalPlan } from "./liveNaturalReview";
import { renderLogMsg } from "./logMsg";

interface Config { fault?: "crash"; recovery?: { sessionId: string; cwd: string }; backgroundProbeSeconds?: number; scenario?: "fixed" | "natural"; request?: string; root: string; cwd: string; provider: ProviderId; model: string; tier: string | null; engine?: "v2" | "v3" | null; cliPath?: string | null; versionPrefix?: string | null }
const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
async function until<T>(read: () => T | null | undefined | false, what: string, ms = 120_000): Promise<T> {
  const end = Date.now() + ms;
  while (Date.now() < end) { const value = read(); if (value) return value; await delay(200); }
  throw new Error(`Timed out waiting for ${what}`);
}
async function click(label: string) {
  const button = await until(() => [...document.querySelectorAll<HTMLButtonElement>("button")]
    .find((b) => b.textContent?.trim() === label && !b.disabled && b.getClientRects().length > 0), `UI button: ${label}`);
  button.click();
}
function plan(config: Config): Plan {
  const verification = (command: string) => config.provider === "REASONIX" ? `After editing, run ${command} using Bash to satisfy Reasonix native verification. ` : "Do not run shell commands. ";
  return {
    version: 1, isUserEdited: true,
    spec: { raw: "Two dependent tasks exercising real CLI execution and Git delivery.", goal: "Fix addition and compose the upstream result.", inScope: ["calc.mjs", "compose.mjs"], outOfScope: ["Changing tests, package.json, credentials or any unrelated file"], decisions: ["Use the explicitly selected CLI/model with no fallback."] },
    contracts: [], notes: config.provider === "REASONIX" ? "Use file tools to edit only the declared deliverable. Run the task acceptance command through Bash after editing, as required by Reasonix. Tokage also runs independent acceptance gates." : "Do not run shell commands yourself; Tokage runs the acceptance scripts. Use file tools only to edit the declared deliverable.", rawMarkdown: "",
    tasks: [
      { id: "T1", domain: "backend", title: "Fix addition", goal: "Read calc.mjs and fix add(a,b) to return a+b. Edit only calc.mjs. Do not modify tests. " + verification("npm run test:add") + "Keep the export. Report using the required TaskReport headings and report contract deviations as None.", depends_on: [], contracts: [], write_scope: ["calc.mjs"], deliverables: ["calc.mjs"], acceptance: { commands: ["npm run test:add"], criteria: ["add(2,3)=5 and add(-2,3)=1"] }, size: "S", difficulty: 1, assigned_model: `${config.provider}:${config.model}`, model_params: { tier: config.tier } },
      { id: "T2", domain: "backend", title: "Compose upstream addition", goal: "Read the upstream calc.mjs after T1. Fix only compose.mjs: import add from ./calc.mjs and implement sumTwice(a,b) as add(a,b)*2. Do not modify calc.mjs, tests or package.json. " + verification("npm run test:integration") + "Report using the required TaskReport headings and report contract deviations as None.", depends_on: ["T1"], contracts: [], write_scope: ["compose.mjs"], deliverables: ["compose.mjs"], acceptance: { commands: ["npm run test:integration"], criteria: ["sumTwice(2,3)=10 and sumTwice(-2,3)=2 using T1's add"] }, size: "S", difficulty: 1, assigned_model: `${config.provider}:${config.model}`, model_params: { tier: config.tier } },
    ],
  };
}

export async function runLiveDesktopAcceptance() {
  const config = await invoke<Config | null>("get_live_acceptance_config");
  if (!config) return;
  let sessionId = "";
  let deliveryCheckout = config.cwd;
  let planning: unknown = null;
  let reviewedPlan: Plan | null = null;
  let stopped = false;
  let faultSeen = false;
  let unlisten: (() => void) | undefined;
  let unsubscribe: (() => void) | undefined;
  const native = { done: 0, tokens: 0, tools: 0, premiumUsageEvents: 0, premiumRequests: 0, creditUsageEvents: 0, credits: 0, errors: [] as string[], models: [] as string[] };
  const worktrees: Record<string, TaskWorktreeInfo> = {};
  const transitions: { at: number; taskStates: Record<string, string>; delivery?: string }[] = [];
  const report = (payload: object) => invoke("report_live_acceptance", { payload: { provider: config.provider, model: config.model, tier: config.tier, engine: config.engine, sessionId, scenario: config.scenario ?? "fixed", originalRequest: config.request, deliveryCheckout, planning, reviewedPlan, ...payload } });
  const snapshot = () => {
    const p = useExecution.getState().pipelines[sessionId];
    return p ? {
      status: p.status, tasks: p.tasks, taskStates: p.taskStates, taskProviders: p.taskProviders, taskCwds: p.taskCwds,
      taskStartedAt: p.taskStartedAt, taskEndedAt: p.taskEndedAt, threadIds: p.threadIds, assignments: p.assignments,
      taskTurnIds: p.taskTurnIds, recoveryTasks: p.recoveryTasks, completionReceipts: p.completionReceipts,
      planDbId: p.planDbId, taskDbIds: p.taskDbIds, integration: p.integration, delivery: p.delivery,
      gateResults: p.gateResults, taskLogs: Object.fromEntries(Object.entries(p.taskLogs).map(([id, logs]) => [id, logs.map(renderLogMsg)])),
    } : null;
  };
  try {
    await until(() => document.querySelector("#root")?.childElementCount, "desktop render");
    await i18n.changeLanguage("en");
    useSettings.setState({ welcomeSeen: true, language: "en", debugProvider: DEFAULT_DEBUG_PROVIDER, activeQuotaQuery: false,
      isolation: "worktree", maxParallel: config.scenario === "natural" ? 2 : 1, requireReview: true, planningAutoApproveBelowUsd: 0, requestMode: config.scenario === "natural" ? "plan" : "fixed", contextMode: "off",
      providerPaths: config.cliPath ? { [config.provider]: config.cliPath } : {},
      providerEnabled: { [config.provider]: true }, providerTrust: { [config.provider]: true },
      modelBanningLevel: "off", blockedModels: [], agentIdleTimeoutMin: 2, agentHardTimeoutMin: 4, planBudgetUsd: 0, pauseOnOverrun: false });
    await until(() => !useProviders.getState().scanning && useProviders.getState().providers.some((p) => p.id === config.provider), "CLI detection");
    if (config.cliPath) await useProviders.getState().refreshOne(config.provider);
    if (config.provider === "REASONIX" && useProviders.getState().realProviders.find((p) => p.id === config.provider)?.status === "UNKNOWN_AUTH") await useProviders.getState().verifyReasonixKey();
    const provider = useProviders.getState().realProviders.find((p) => p.id === config.provider);
    if (provider?.status !== "READY" || !provider.chat || provider.debugVirtual) throw new Error(`Actual CLI not READY: ${provider?.status}`);
    if (config.versionPrefix && (config.scenario === "natural" ? provider.version !== config.versionPrefix : !provider.version?.startsWith(config.versionPrefix))) throw new Error(`Wrong CLI version: ${provider.version}`);
    if (useSettings.getState().debugProvider.output !== "off") throw new Error("Debug Provider must be disabled");
    await report({ status: "running", step: "real-provider-ready", version: provider.version });
    if (config.backgroundProbeSeconds) {
      const started = Date.now();
      await report({ status: "running", step: "background-probe-start" });
      await delay(config.backgroundProbeSeconds * 1000);
      await report({ status: "running", step: "background-probe-passed", backgroundElapsedMs: Date.now() - started });
    }
    if (config.recovery) {
      sessionId = config.recovery.sessionId;
      deliveryCheckout = config.recovery.cwd;
      await ensureSubAgentEventListener();
      const loaded = useExecution.getState().pipelines[sessionId] ?? await useExecution.getState().loadPipelineFromDisk(deliveryCheckout, sessionId);
      if (!loaded || !loaded.recoveryTasks?.length) throw new Error(`No interrupted durable pipeline recovered: ${JSON.stringify({ loaded: loaded && { states: loaded.taskStates, recoveryTasks: loaded.recoveryTasks }, current: snapshot() })}`);
      const before = snapshot();
      const evidence: Record<string, unknown> = {};
      for (const id of loaded.recoveryTasks) {
        const turnId = loaded.taskTurnIds?.[id];
        if (!turnId) throw new Error("Recovered attempt identity missing");
        evidence[id] = await taskRecoveryEvidence(turnId, loaded.taskCwds?.[id] ?? deliveryCheckout, loaded.taskProviders?.[id], loaded.threadIds?.[id]);
        await Promise.all([continueTask(sessionId, id), continueTask(sessionId, id)]);
      }
      await flushPipelines(); await delay(2000);
      const p = useExecution.getState().pipelines[sessionId];
      if (loaded.recoveryTasks.some((id) => p.taskStates[id] !== "BLOCKED")) throw new Error("Uncertain CLI outcome was not blocked");
      if (Object.entries(evidence).some(([, e]) => !(e as { runtimeIdle: boolean }).runtimeIdle)) throw new Error("Startup left a native process active");
      if (p.delivery?.phase === "merged" || p.delivery?.phase === "ready") throw new Error("Uncertain recovery allowed delivery");
      stopped = true;
      await report({ status: "passed", step: "crash-recovered-blocked", before, evidence, snapshot: snapshot() });
      return;
    }
    useUi.getState().closeSettings();
    useProjects.getState().addProject(config.cwd);
    const projectId = useProjects.getState().activeProjectId!;
    useProjects.getState().setProjectTrust(projectId, true);
    const sessionWorktree = config.scenario === "natural"
      ? await createSessionWorktree(config.cwd, `natural-${Date.now().toString(36)}`) : undefined;
    deliveryCheckout = sessionWorktree?.cwd ?? config.cwd;
    sessionId = useProjects.getState().createSession(projectId, `Live acceptance ${config.provider}`, sessionWorktree);
    useSettings.getState().set("tokageGitignore", { [projectId]: true });
    const ref = { provider: config.provider, model: config.model };
    useChat.getState().setLockedModel(sessionId, ref);
    if (config.tier) useChat.getState().setModelOptions(ref, { tier: config.tier });
    // Fixture-only authorization is explicit and expires with this app session.
    useAuth.getState().grantFullAuto(sessionId, "session");
    await ensureSubAgentEventListener();
    unlisten = await onChatEvent((event) => {
      if (!event.turnId.startsWith(`subagent-${sessionId}-`)) return;
      if (event.kind === "done") {
        if (event.credits != null && Number.isFinite(event.credits) && event.credits >= 0) { native.creditUsageEvents++; native.credits += event.credits; }
        native.done++; native.tokens += event.inputTokens + event.outputTokens + (event.cacheReadTokens ?? 0);
        if (event.premiumRequests != null && Number.isFinite(event.premiumRequests) && event.premiumRequests >= 0) {
          native.premiumUsageEvents++; native.premiumRequests += event.premiumRequests;
        }
      }
      if (event.kind === "toolStart") { native.tools++; faultSeen = true; }
      if (event.kind === "error") native.errors.push(event.message);
      if (event.kind === "model") native.models.push(event.name);
      if (["thread", "toolStart", "done", "error", "approvalRequest"].includes(event.kind))
        void report({ status: "running", step: `native-${event.kind}`, turnId: event.turnId, native: { ...native } });
    });
    let signature = "";
    unsubscribe = useExecution.subscribe((state) => {
      const p = state.pipelines[sessionId]; if (!p || stopped) return;
      for (const [id, wt] of Object.entries(p.taskWorktrees ?? {})) if (wt) worktrees[id] = wt;
      const next = JSON.stringify([p.taskStates, p.taskCwds, p.threadIds, p.delivery?.phase]);
      if (next === signature) return; signature = next;
      transitions.push({ at: Date.now(), taskStates: { ...p.taskStates }, delivery: p.delivery?.phase });
      void report({ status: "running", step: "pipeline-transition", snapshot: snapshot() });
    });
    await delay(300);
    if (useUi.getState().trustPromptOpen) await click(i18n.t("settings.cli.trust.dialog.done"));
    if (config.scenario === "natural") {
      if (!config.request) throw new Error("Natural acceptance needs the original request");
      useChat.getState().setPick("plan", ref);
      useUi.getState().setWorkspaceTab("chat");
      await click(i18n.t("workspace.composer.modes.plan"));
      const textarea = await until(() => document.querySelector<HTMLTextAreaElement>('textarea[aria-describedby="composer-mode-desc"]'), "request composer");
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
      setter.call(textarea, config.request);
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
      await delay(200);
      const send = await until(() => [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.getAttribute("aria-label") === i18n.t("workspace.composer.send") && !b.disabled), "send natural request");
      send.click();
      const message = await until(() => {
        const m = useChat.getState().conversations[sessionId]?.messages.find((m) => m.role === "assistant");
        if (m && m.mode !== "plan") throw new Error(`Composer dispatched ${m.mode} instead of planning`);
        if (m?.status === "waiting_approval") throw new Error("Planning requires interactive approval; no automatic approval fallback");
        if (m?.status === "cancelled") throw new Error("Planning was cancelled");
        if (m?.status === "done" && !m.text.trim()) throw new Error("Planning completed without a textual plan");
        if (m?.status === "error" || m?.planError) throw new Error(`Planning failed: ${m.error ?? m.planError}`);
        return m?.status === "done" && m.plan ? m : null;
      }, "real natural-language planning", 5 * 60_000);
      planning = message;
      // The rendered review loads and parses the real planner reply through production code.
      useUi.getState().setWorkspaceTab("plan");
      reviewedPlan = await parsePlanningOutput({ markdown: message.text });
      reviewNaturalPlan(reviewedPlan);
      await until(() => [...document.querySelectorAll<HTMLButtonElement>("button")].some((b) => b.textContent?.trim() === i18n.t("plan.review.confirmExecute") && !b.disabled), "generated plan review");
      useChat.getState().setLockedModel(sessionId, ref);
      await report({ status: "running", step: "generated-plan-reviewed" });
    } else {
      await persistPlan(config.cwd, sessionId, plan(config));
    }
    useUi.getState().setWorkspaceTab("plan");
    await click(i18n.t("plan.review.confirmExecute"));
    await click(i18n.t("plan.review.confirmModal.confirmStart"));
    await until(() => useExecution.getState().pipelines[sessionId], "UI approval / persisted plan / dispatch");
    await delay(800); // PlanReview switches back to chat after approval.
    useUi.getState().setWorkspaceTab("pipeline");
    if (config.fault === "crash") {
      await until(() => faultSeen, "real CLI tool event before crash", 180_000);
      await flushPipelines();
      const p = useExecution.getState().pipelines[sessionId];
      if (Object.keys(p.completionReceipts ?? {}).length || Object.values(p.taskStates).includes("DONE")) throw new Error("Crash trigger arrived after completion; refuse invalid fault evidence");
      await report({ status: "running", step: "crash-ready", snapshot: snapshot() });
      await delay(30_000);
      throw new Error("External crash injection did not arrive");
    }
    await until(() => {
      const p = useExecution.getState().pipelines[sessionId];
      if (Object.values(p?.taskStates ?? {}).some((s) => s === "BLOCKED")) throw new Error(`Pipeline blocked: ${JSON.stringify(snapshot())}`);
      if (p?.delivery?.phase === "failed") throw new Error(`Delivery failed: ${p.delivery.error}`);
      return p?.status === "DONE" && p.delivery?.phase === "ready" ? p : null;
    }, "two real CLI tasks / gates / commits / integration", 10 * 60_000);
    const p = useExecution.getState().pipelines[sessionId];
    if (Object.values(p.taskStates).some((s) => s !== "DONE") || p.delivery?.fullGatePassed !== true) throw new Error("Tasks or full delivery gate failed");
    if (native.done < 2 || (config.provider === "COPILOT_CLI" ? native.premiumUsageEvents < 2 : config.provider === "KIRO_CLI" ? native.creditUsageEvents < 2 : native.tokens <= 0) || native.tools <= 0 || native.errors.length) throw new Error(`Missing successful actual CLI evidence: ${JSON.stringify(native)}`);
    if (p.tasks.some((t) => p.taskProviders?.[t.id] !== config.provider)) throw new Error("Provider fallback detected");
    if (p.tasks.some((t) => !p.threadIds?.[t.id] || !worktrees[t.id]) || new Set(Object.values(worktrees).map((w) => w.root)).size !== p.tasks.length) throw new Error("Missing isolated native sessions/worktrees");
    if (p.tasks.some((t) => t.depends_on.some((d) => (p.taskStartedAt?.[t.id] ?? 0) < (p.taskEndedAt?.[d] ?? Number.MAX_SAFE_INTEGER)))) throw new Error("DAG dependency ordering violated");
    if (config.scenario === "natural") {
      await report({ status: "running", step: "independent-review-required", native, worktrees, transitions, snapshot: snapshot() });
      let review: { passed: boolean; error?: string; head?: string } | null = null;
      const end = Date.now() + 60_000;
      while (!review && Date.now() < end) {
        review = await invoke("get_live_acceptance_review");
        if (!review) await delay(200);
      }
      if (!review?.passed) throw new Error(`Independent review refused delivery: ${review?.error ?? "timeout"}`);
      if (review.head !== p.delivery.evidence?.snapshot.head) throw new Error("Independent review commit differs from accepted commit");
    }
    const originalConfirm = window.confirm;
    window.confirm = (message) => { if (message !== i18n.t("pipeline.mergeConfirm")) throw new Error("Unexpected confirmation"); return true; };
    try {
      await click(i18n.t("pipeline.delivery.merge"));
      await until(() => {
        const delivery = useExecution.getState().pipelines[sessionId]?.delivery;
        if (delivery?.error) throw new Error(`Merge refused: ${renderLogMsg(delivery.error)}`);
        return delivery?.phase === "merged";
      }, "UI merge to fixture checkout");
    }
    finally { window.confirm = originalConfirm; }
    await flushPipelines(); await delay(600);
    stopped = true;
    await report({ status: "passed", step: "desktop-ui-dag-merged", native, worktrees, transitions, snapshot: snapshot() });
  } catch (error) {
    planning ??= useChat.getState().conversations[sessionId]?.messages.find((m) => m.role === "assistant") ?? null;
    stopped = true;
    const failureSnapshot = snapshot();
    useExecution.getState().pauseExecution(sessionId);
    await useChat.getState().cancel(sessionId);
    const failedPipeline = useExecution.getState().pipelines[sessionId];
    if (failedPipeline?.delivery?.phase === "running_gates") await cancelPlanAcceptance(sessionId);
    for (const task of failedPipeline?.tasks ?? []) await cancelTask(sessionId, task.id);
    await report({ status: "failed", error: String(error), native, worktrees, transitions, snapshot: failureSnapshot, stoppedSnapshot: snapshot(), uiText: document.body.innerText.slice(-12_000) });
  } finally { unlisten?.(); unsubscribe?.(); }
}
