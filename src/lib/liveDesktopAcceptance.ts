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
import { ensureSubAgentEventListener } from "./taskRunner";
import { onChatEvent, persistPlan, type Plan, type ProviderId, type TaskWorktreeInfo } from "./ipc";
import { renderLogMsg } from "./logMsg";

interface Config { root: string; cwd: string; provider: ProviderId; model: string; tier: string | null; engine?: "v2" | "v3" | null }
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
  return {
    version: 1, isUserEdited: true,
    spec: { raw: "Two dependent tasks exercising real CLI execution and Git delivery.", goal: "Fix addition and compose the upstream result.", inScope: ["calc.mjs", "compose.mjs"], outOfScope: ["Changing tests, package.json, credentials or any unrelated file"], decisions: ["Use the explicitly selected CLI/model with no fallback."] },
    contracts: [], notes: "Do not run shell commands yourself; Tokage runs the acceptance scripts. Use file tools only to edit the declared deliverable.", rawMarkdown: "",
    tasks: [
      { id: "T1", domain: "backend", title: "Fix addition", goal: "Read calc.mjs and fix add(a,b) to return a+b. Edit only calc.mjs. Do not modify tests or run shell commands. Keep the export. Report using the required TaskReport headings and report contract deviations as None.", depends_on: [], contracts: [], write_scope: ["calc.mjs"], deliverables: ["calc.mjs"], acceptance: { commands: ["npm run test:add"], criteria: ["add(2,3)=5 and add(-2,3)=1"] }, size: "S", difficulty: 1, assigned_model: `${config.provider}:${config.model}`, model_params: { tier: config.tier } },
      { id: "T2", domain: "backend", title: "Compose upstream addition", goal: "Read the upstream calc.mjs after T1. Fix only compose.mjs: import add from ./calc.mjs and implement sumTwice(a,b) as add(a,b)*2. Do not modify calc.mjs, tests or package.json; do not run shell commands. Report using the required TaskReport headings and report contract deviations as None.", depends_on: ["T1"], contracts: [], write_scope: ["compose.mjs"], deliverables: ["compose.mjs"], acceptance: { commands: ["npm run test:integration"], criteria: ["sumTwice(2,3)=10 and sumTwice(-2,3)=2 using T1's add"] }, size: "S", difficulty: 1, assigned_model: `${config.provider}:${config.model}`, model_params: { tier: config.tier } },
    ],
  };
}

export async function runLiveDesktopAcceptance() {
  const config = await invoke<Config | null>("get_live_acceptance_config");
  if (!config) return;
  let sessionId = "";
  let stopped = false;
  let unlisten: (() => void) | undefined;
  let unsubscribe: (() => void) | undefined;
  const native = { done: 0, tokens: 0, tools: 0, premiumUsageEvents: 0, premiumRequests: 0, creditUsageEvents: 0, credits: 0, errors: [] as string[], models: [] as string[] };
  const worktrees: Record<string, TaskWorktreeInfo> = {};
  const transitions: { at: number; taskStates: Record<string, string>; delivery?: string }[] = [];
  const report = (payload: object) => invoke("report_live_acceptance", { payload: { provider: config.provider, model: config.model, tier: config.tier, engine: config.engine, sessionId, ...payload } });
  const snapshot = () => {
    const p = useExecution.getState().pipelines[sessionId];
    return p ? {
      status: p.status, tasks: p.tasks, taskStates: p.taskStates, taskProviders: p.taskProviders, taskCwds: p.taskCwds,
      taskStartedAt: p.taskStartedAt, taskEndedAt: p.taskEndedAt, threadIds: p.threadIds, assignments: p.assignments,
      planDbId: p.planDbId, taskDbIds: p.taskDbIds, integration: p.integration, delivery: p.delivery,
      gateResults: p.gateResults, taskLogs: Object.fromEntries(Object.entries(p.taskLogs).map(([id, logs]) => [id, logs.map(renderLogMsg)])),
    } : null;
  };
  try {
    await until(() => document.querySelector("#root")?.childElementCount, "desktop render");
    await i18n.changeLanguage("en");
    useSettings.setState({ welcomeSeen: true, language: "en", debugProvider: DEFAULT_DEBUG_PROVIDER, activeQuotaQuery: false,
      isolation: "worktree", maxParallel: 1, requireReview: true, planningAutoApproveBelowUsd: 0, requestMode: "fixed", contextMode: "off",
      providerEnabled: { [config.provider]: true }, providerTrust: { [config.provider]: true },
      modelBanningLevel: "off", blockedModels: [], agentIdleTimeoutMin: 2, agentHardTimeoutMin: 4, planBudgetUsd: 0, pauseOnOverrun: false });
    await until(() => !useProviders.getState().scanning && useProviders.getState().providers.some((p) => p.id === config.provider), "CLI detection");
    const provider = useProviders.getState().realProviders.find((p) => p.id === config.provider);
    if (provider?.status !== "READY" || !provider.chat || provider.debugVirtual) throw new Error(`Actual CLI not READY: ${provider?.status}`);
    if (useSettings.getState().debugProvider.output !== "off") throw new Error("Debug Provider must be disabled");
    await report({ status: "running", step: "real-provider-ready", version: provider.version });
    useProjects.getState().addProject(config.cwd);
    const projectId = useProjects.getState().activeProjectId!;
    useProjects.getState().setProjectTrust(projectId, true);
    sessionId = useProjects.getState().createSession(projectId, `Live acceptance ${config.provider}`);
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
      if (event.kind === "toolStart") native.tools++;
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
    await persistPlan(config.cwd, sessionId, plan(config));
    useUi.getState().setWorkspaceTab("plan");
    await click(i18n.t("plan.review.confirmExecute"));
    await click(i18n.t("plan.review.confirmModal.confirmStart"));
    await until(() => useExecution.getState().pipelines[sessionId], "UI approval / persisted plan / dispatch");
    await delay(800); // PlanReview switches back to chat after approval.
    useUi.getState().setWorkspaceTab("pipeline");
    await until(() => {
      const p = useExecution.getState().pipelines[sessionId];
      if (Object.values(p?.taskStates ?? {}).some((s) => s === "BLOCKED")) throw new Error(`Pipeline blocked: ${JSON.stringify(snapshot())}`);
      if (p?.delivery?.phase === "failed") throw new Error(`Delivery failed: ${p.delivery.error}`);
      return p?.status === "DONE" && p.delivery?.phase === "ready" ? p : null;
    }, "two real CLI tasks / gates / commits / integration", 10 * 60_000);
    const p = useExecution.getState().pipelines[sessionId];
    if (p.taskStates.T1 !== "DONE" || p.taskStates.T2 !== "DONE" || p.delivery?.fullGatePassed !== true) throw new Error("Tasks or full delivery gate failed");
    if (native.done < 2 || (config.provider === "COPILOT_CLI" ? native.premiumUsageEvents < 2 : config.provider === "KIRO_CLI" ? native.creditUsageEvents < 2 : native.tokens <= 0) || native.tools <= 0 || native.errors.length) throw new Error(`Missing successful actual CLI evidence: ${JSON.stringify(native)}`);
    if (p.taskProviders?.T1 !== config.provider || p.taskProviders?.T2 !== config.provider) throw new Error("Provider fallback detected");
    if (!p.threadIds?.T1 || !p.threadIds?.T2 || !worktrees.T1 || !worktrees.T2 || worktrees.T1.root === worktrees.T2.root) throw new Error("Missing isolated native sessions/worktrees");
    if ((p.taskStartedAt?.T2 ?? 0) < (p.taskEndedAt?.T1 ?? Number.MAX_SAFE_INTEGER)) throw new Error("DAG dependency ordering violated");
    const originalConfirm = window.confirm;
    window.confirm = (message) => { if (message !== i18n.t("pipeline.mergeConfirm")) throw new Error("Unexpected confirmation"); return true; };
    try { await click(i18n.t("pipeline.delivery.merge")); await until(() => useExecution.getState().pipelines[sessionId]?.delivery?.phase === "merged", "UI merge to fixture checkout"); }
    finally { window.confirm = originalConfirm; }
    await flushPipelines(); await delay(600);
    stopped = true;
    await report({ status: "passed", step: "desktop-ui-dag-merged", native, worktrees, transitions, snapshot: snapshot() });
  } catch (error) {
    stopped = true;
    await report({ status: "failed", error: String(error), native, worktrees, transitions, snapshot: snapshot(), uiText: document.body.innerText.slice(-12_000) });
  } finally { unlisten?.(); unsubscribe?.(); }
}
