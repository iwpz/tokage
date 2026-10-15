import { builtinDebugCatalog } from "../lib/debugProvider";
import { DemoChecks } from "./workspace/DemoChecks";
import { demoIdentity, DEMO_PROJECT_ID } from "../lib/demoIdentity";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowUp, ChevronDown, ListPlus, Folder, FolderPlus, GitFork, MessagesSquare, Network, Square, Workflow, ScrollText } from "lucide-react";
import { cn } from "../lib/cn";
import { createSessionWorktree, pickProjectDirectory, triageRequest, type TriageResult, type TurnMode } from "../lib/ipc";
import { directBlocked, optionSummary, recommend, sameRef, selectableGroups, triage, type ModelRef } from "../lib/models";
import { dispatchAsync } from "../lib/dispatch";
import { planTurn } from "../lib/turn";
import { sessionCwd, useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { isStreaming, useChat } from "../stores/chat";
import { useSettings, type RequestMode } from "../stores/settings";
import { useUi, type WorkspaceTab } from "../stores/ui";
import { useGitInfo } from "../lib/useGitInfo";
import { ProjectMenu } from "./ProjectMenu";
import { ModelPicker } from "./ModelPicker";
import { ModelLabel } from "./ModelLabel";
import { locateSpec, optionsOf, parseSpec } from "../lib/modelSpec";
import { Conversation } from "./Conversation";
import { MessageQueue } from "./MessageQueue";
import { PlanReview } from "./planning/PlanReview";
import { PipelineView } from "./planning/PipelineView";
import { GitStatusBar } from "./GitBar";
import { ContextBar } from "./workspace/ContextBar";
import { SessionHistoryView } from "./workspace/SessionHistoryView";
import {
  loadPlan,
  type Plan,
  type PlanDiff,
} from "../lib/ipc";
import { calculateSessionCacheStatus } from "../lib/cachePolicy";
import {
  getLastDirectPrefer,
  notifyReplanReady,
  routePipelineAppend,
  setReplanReadyHandler,
} from "../lib/appendTriage";
import { confirmReplan, replanSession, startReplanTurn } from "../lib/replan";
import { continueInterrupted, interruptExecution, supplementTask } from "../lib/taskRunner";
import { askAboutPipeline } from "../lib/pipelineAsk";
import { PlanDiffView } from "./planning/PlanDiffView";
import { Logo } from "./Logo";
import { Button, Modal } from "./ui";
import { useExecution } from "../stores/execution";

export function Workspace() {
  const { projects, activeProjectId } = useProjects();
  const project = projects.find((p) => p.id === activeProjectId) ?? null;

  return (
    project ? <ProjectWorkspace /> : <Welcome />
  );
}

function Welcome() {
  const { t } = useTranslation();
  const addProject = useProjects((s) => s.addProject);

  return (
    <div className="flex flex-1 items-center justify-center p-8">
      <div className="max-w-md text-center">
        <Logo className="mx-auto size-12" />
        <h1 className="mt-5 text-xl font-semibold">{t("workspace.welcomeTitle")}</h1>
        <p className="mt-2 text-[13px] leading-relaxed text-muted">{t("workspace.welcomeBody")}</p>
        <Button
          variant="primary"
          className="mt-6"
          onClick={async () => {
            const path = await pickProjectDirectory();
            if (path) addProject(path);
          }}
        >
          <FolderPlus className="size-4" />
          {t("sidebar.addProject")}
        </Button>
      </div>
    </div>
  );
}

const TAB_ICONS: Record<WorkspaceTab, typeof Network> = {
  chat: MessagesSquare,
  plan: Network,
  pipeline: Workflow,
  session: ScrollText,
};

function ProjectWorkspace() {
  const { t } = useTranslation();
  const activeProjectId = useProjects((s) => s.activeProjectId);
  const activeSessionId = useProjects((s) => s.activeSessionId);
  const { workspaceTab, setWorkspaceTab } = useUi();
  const contextMode = useSettings((s) => s.contextMode);
  const configuredDebugOutput = useSettings((s) => s.debugProvider.output);
  const isDemo = activeProjectId === DEMO_PROJECT_ID || !!demoIdentity(activeSessionId);
  const debugOutput = isDemo ? "mock" : configuredDebugOutput;
  const conv = useChat((s) => (activeSessionId ? s.conversations[activeSessionId] : undefined));

  const [replanDiff, setReplanDiff] = useState<{
    current: Plan;
    proposed: Plan;
    diff: PlanDiff;
  } | null>(null);
  const [replanBusy, setReplanBusy] = useState(false);

  useEffect(() => {
    setReplanReadyHandler((payload) => {
      if (payload.sessionId !== activeSessionId) return;
      setReplanDiff({
        current: payload.current,
        proposed: payload.proposed,
        diff: payload.diff,
      });
    });
    return () => setReplanReadyHandler(null);
  }, [activeSessionId]);

  function formatProviderName(pid: string) {
    switch (pid) {
      case "CLAUDE_CODE":
        return "Claude Code";
      case "CODEX_CLI":
        return "Codex CLI";
      case "CURSOR_CLI":
        return "Cursor CLI";
      case "COPILOT_CLI":
        return "Copilot";
      case "ANTIGRAVITY_CLI":
        return "Antigravity";
      case "AIDER":
        return "Aider";
      case "OPENCODE":
        return "OpenCode";
      case "REASONIX":
        return "Reasonix";
      default:
        return pid;
    }
  }

  const contextStats = useMemo(() => {
    if (!conv?.messages?.length) {
      return {
        tokens: 0,
        maxTokens: 128_000,
        modelName: undefined,
        breakdown: [],
        cacheType: "none" as const,
        isWarm: false,
        ttlRemaining: 0,
        elapsedSeconds: 0,
        ttlSeconds: 0,
      };
    }
    const lastMsg = conv.messages[conv.messages.length - 1];
    const reportedContext =
      lastMsg.usage?.contextTokens ??
      (lastMsg.usage ? lastMsg.usage.input + (lastMsg.usage.cacheRead ?? 0) : 0);
    const tokens = lastMsg.debugRoute?.output === "mock" ? 0 : reportedContext > 0 ? reportedContext : 25_000 + conv.messages.length * 4_000;

    // Detect model and its effective context limit
    const lastAssistantMsg = [...conv.messages].reverse().find((m) => m.role === "assistant");
    const activeModelName = lastAssistantMsg?.modelName ?? lastAssistantMsg?.model ?? conv.thread?.ref.model;
    const providerKey = lastAssistantMsg?.provider ?? lastAssistantMsg?.ref?.provider ?? conv.thread?.ref.provider;
    const modelLower = (activeModelName ?? "").toLowerCase();

    // Determine maxTokens dynamically (Claude 1M -> 1,000,000; Claude Sonnet/Opus -> 200,000; etc.)
    let maxTokens = 128_000;
    if (modelLower.includes("1m")) {
      maxTokens = 1_000_000;
    } else if (modelLower.includes("claude") || modelLower.includes("sonnet") || modelLower.includes("opus")) {
      maxTokens = 200_000;
    } else if (modelLower.includes("gemini")) {
      maxTokens = 1_000_000;
    }

    // Breakdown per sub-CLI provider across session
    const map = new Map<string, { label: string; tokens: number; costUsd: number; turns: number }>();
    for (const m of conv.messages) {
      if (m.role !== "assistant") continue;
      const pKey = m.provider ?? m.ref?.provider ?? "OTHER";
      const label = formatProviderName(pKey);
      const cur = map.get(pKey) ?? { label, tokens: 0, costUsd: 0, turns: 0 };
      const turnTokens = m.debugRoute?.output === "mock" ? 0 : m.usage ? m.usage.input + m.usage.output + (m.usage.cacheRead ?? 0) : 4_000;
      cur.tokens += turnTokens;
      cur.costUsd += m.usage?.costUsd ?? 0;
      cur.turns += 1;
      map.set(pKey, cur);
    }

    const breakdown = Array.from(map.entries()).map(([provider, data]) => ({
      provider,
      ...data,
    }));

    const cacheStatus = calculateSessionCacheStatus(
      lastAssistantMsg?.endedAt,
      activeModelName,
      providerKey,
    );

    return {
      tokens,
      maxTokens,
      modelName: activeModelName,
      breakdown,
      cacheType: cacheStatus.cacheType,
      isWarm: cacheStatus.isWarm,
      ttlRemaining: cacheStatus.ttlRemainingSeconds,
      elapsedSeconds: cacheStatus.elapsedSeconds,
      ttlSeconds: cacheStatus.ttlSeconds,
    };
  }, [conv]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {isDemo && <div role="note" className="border-b border-border bg-accent-soft px-4 py-2 text-xs text-accent">{t("demo.banner")}</div>}
      {debugOutput !== "off" && <div role="status" className="border-b border-border bg-accent-soft px-4 py-2 text-xs text-accent">
        {t("debugProvider.banner", { source: t(`debugProvider.outputs.${debugOutput}`) })}
        {debugOutput === "mock" ? t("debugProvider.mockBanner") : t("debugProvider.realBanner")}
      </div>}
      {isDemo && activeSessionId && <DemoChecks sessionId={activeSessionId} />}
      {activeSessionId ? (
        <>
          <div role="tablist" aria-label="Views" className="flex gap-1 border-b border-border px-4 py-1.5">
            {(Object.keys(TAB_ICONS) as WorkspaceTab[]).map((tab) => {
              const Icon = TAB_ICONS[tab];
              return (
                <button
                  key={tab}
                  type="button"
                  role="tab"
                  aria-selected={workspaceTab === tab}
                  onClick={() => setWorkspaceTab(tab)}
                  className={cn(
                    "inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[13px]",
                    workspaceTab === tab ? "bg-active text-fg" : "text-muted hover:bg-hover hover:text-fg",
                  )}
                >
                  <Icon className="size-3.5" />
                  {t(`workspace.tabs.${tab}`)}
                </button>
              );
            })}
          </div>
          {contextMode !== "off" && (
            <ContextBar
              tokens={contextStats.tokens}
              maxTokens={contextStats.maxTokens}
              modelName={contextStats.modelName}
              breakdown={contextStats.breakdown}
              cacheType={contextStats.cacheType}
              isWarm={contextStats.isWarm}
              ttlRemainingSeconds={contextStats.ttlRemaining}
            />
          )}
          {workspaceTab === "chat" ? (
            <Conversation sessionId={activeSessionId} />
          ) : workspaceTab === "plan" ? (
            <PlanReview
              key={activeSessionId}
              sessionId={activeSessionId}
              onSwitchTab={setWorkspaceTab}
              onRequestReplan={(feedback) => {
                void (async () => {
                  if (!activeSessionId) return;
                  setWorkspaceTab("chat");
                  setReplanBusy(true);
                  try {
                    const project = useProjects
                      .getState()
                      .projects.find((p) => p.sessions.some((s) => s.id === activeSessionId));
                    if (!project) return;
                    const cwd = sessionCwd(project, activeSessionId);
                    const current = await loadPlan(cwd, activeSessionId);
                    const result = await startReplanTurn(activeSessionId, current, feedback);
                    if ("error" in result) {
                      useChat.getState().postSystemMessage(
                        activeSessionId,
                        t("workspaceMsg.replanError", { error: result.error }),
                      );
                      return;
                    }
                    setReplanDiff({
                      current,
                      proposed: result.proposed,
                      diff: result.diff,
                    });
                  } catch (err) {
                    useChat.getState().postSystemMessage(
                      activeSessionId,
                      t("workspaceMsg.replanFailed", { err: String(err) }),
                    );
                  } finally {
                    setReplanBusy(false);
                  }
                })();
              }}
            />
          ) : workspaceTab === "pipeline" ? (
            <PipelineView sessionId={activeSessionId} onSwitchTab={setWorkspaceTab} />
          ) : (
            <SessionHistoryView sessionId={activeSessionId} projectId={activeProjectId} />
          )}
          {replanDiff && activeSessionId && (
            <PlanDiffView
              diff={replanDiff.diff}
              busy={replanBusy}
              onCancel={() => setReplanDiff(null)}
              onConfirm={async () => {
                setReplanBusy(true);
                try {
                  const result = await confirmReplan({
                    sessionId: activeSessionId,
                    currentPlan: replanDiff.current,
                    proposed: replanDiff.proposed,
                    diff: replanDiff.diff,
                  });
                  if ("error" in result) {
                    useChat.getState().postSystemMessage(
                      activeSessionId,
                      t("workspaceMsg.replanError", { error: result.error }),
                    );
                  } else {
                    useChat.getState().postSystemMessage(
                      activeSessionId,
                      t("workspaceMsg.replanApplied", { version: result.plan.version ?? "?", count: result.adaptationTasks.length }),
                    );
                    setWorkspaceTab("plan");
                    // Tasks interrupted for the replan resume now; redefined ones start fresh.
                    void continueInterrupted(activeSessionId);
                  }
                } finally {
                  setReplanBusy(false);
                  setReplanDiff(null);
                }
              }}
            />
          )}
        </>
      ) : (
        <div className="flex flex-1 items-center justify-center p-8">
          <div className="max-w-lg text-center">
            <h1 className="text-2xl font-semibold">
              {t("workspace.emptyTitleBefore")}
              <ProjectSwitcher />
              {t("workspace.emptyTitleAfter")}
            </h1>
            <p className="mt-2 text-[13px] leading-relaxed text-muted">{t("workspace.emptyBody")}</p>
          </div>
        </div>
      )}
      {(!activeSessionId || workspaceTab === "chat") && <Composer />}
    </div>
  );
}

/** Underlined project name in the empty-state title; click to switch or add projects. */
function ProjectSwitcher() {
  const project = useProjects((s) => s.projects.find((p) => p.id === s.activeProjectId));
  if (!project) return null;
  return (
    <ProjectMenu
      trigger={project.name}
      triggerClassName="rounded underline decoration-subtle decoration-2 underline-offset-4 hover:decoration-fg focus-visible:outline-2 focus-visible:outline-ring"
    />
  );
}

/** Project picker, git status and the worktree option, shown under the composer. */
function ComposerContext({ worktree, onWorktree }: { worktree: boolean; onWorktree: (v: boolean) => void }) {
  const { t } = useTranslation();
  const project = useProjects((s) => s.projects.find((p) => p.id === s.activeProjectId));
  const activeSessionId = useProjects((s) => s.activeSessionId);
  const session = project?.sessions.find((x) => x.id === activeSessionId);
  // Refresh after each turn: the agent may have edited files or switched branches.
  const turns = useChat((s) => (activeSessionId ? s.conversations[activeSessionId]?.messages.length : 0));
  const busy = useChat((s) => (activeSessionId ? isStreaming(s.conversations[activeSessionId]) : false));
  const [gitTick, setGitTick] = useState(0);
  const cwd = project ? sessionCwd(project, activeSessionId) : undefined;
  const git = useGitInfo(cwd, `${turns}:${busy}:${gitTick}`);
  if (!project) return null;

  // An existing session keeps the folder it started in; the checkbox applies to new ones.
  const locked = !!session;
  const checked = session ? !!session.worktree : worktree;
  const canWorktree = !!git?.isRepo && git.hasCommits;

  return (
    <div className="mt-2 flex flex-wrap items-center gap-x-1 gap-y-1 px-1 text-xs text-muted">
      <ProjectMenu
        placement="top"
        trigger={
          <>
            <Folder aria-hidden className="size-3.5" />
            <span className="max-w-48 truncate">{project.name}</span>
            <ChevronDown aria-hidden className="size-3" />
          </>
        }
        triggerClassName="inline-flex h-6 items-center gap-1.5 rounded-md px-1.5 hover:bg-hover hover:text-fg"
      />
      {git?.isRepo && cwd && (
        <GitStatusBar cwd={cwd} git={git} onChanged={() => setGitTick((n) => n + 1)} />
      )}
      {git && !git.isRepo && <span className="px-1.5 text-subtle">{t("workspace.git.notRepo")}</span>}
      {git?.isRepo && (
        <label
          title={
            locked
              ? session?.worktree
                ? t("workspace.git.worktreeAt", { path: session.worktree.root })
                : t("workspace.git.worktreeLocked")
              : canWorktree
                ? t("workspace.git.worktreeHint")
                : t("workspace.git.worktreeNoCommits")
          }
          className={cn(
            "ml-auto inline-flex h-6 items-center gap-1.5 rounded-md px-1.5",
            locked || !canWorktree ? "text-subtle" : "cursor-pointer hover:bg-hover hover:text-fg",
          )}
        >
          <input
            type="checkbox"
            checked={checked && (locked || canWorktree)}
            disabled={locked || !canWorktree}
            onChange={(e) => onWorktree(e.target.checked)}
            className="size-3.5 accent-[var(--accent)]"
          />
          <GitFork aria-hidden className="size-3.5" />
          {t("workspace.git.worktree")}
        </label>
      )}
    </div>
  );
}

const MODES: RequestMode[] = ["auto", "plan", "direct", "fixed"];

/** What a message means while a plan is executing (replaces MODES until the plan is done). */
type ExecMode = "supplement" | "replan" | "ask";
const EXEC_MODES: ExecMode[] = ["supplement", "replan", "ask"];
const LIVE_STATES = ["RUNNING", "VERIFYING", "REPAIRING"];
const FINISHED_STATES = ["DONE", "CANCELLED", "SKIPPED"];

type ExecConfirm =
  | { kind: "supplement"; taskId: string; title: string; live: boolean; text: string }
  | { kind: "replan"; text: string; live: string[] };

function Composer() {
  const { t } = useTranslation();
  const defaultMode = useSettings((s) => (s.rememberMode ? s.lastMode : s.requestMode));
  const setSetting = useSettings((s) => s.set);
  const enabled = useSettings((s) => s.providerEnabled);
  const trust = useSettings((s) => s.providerTrust);
  const blockedModels = useSettings((s) => s.blockedModels);
  const banningLevel = useSettings((s) => s.modelBanningLevel);
  const modelProviderPreferences = useSettings((s) => s.modelProviderPreferences);
  const modelChannelAccess = useSettings((s) => s.modelChannelAccess);
  const providers = useProviders((s) => s.providers);
  const project = useProjects((s) => s.projects.find((p) => p.id === s.activeProjectId));
  const isDemoProject = project?.id === DEMO_PROJECT_ID;
  const activeSessionId = useProjects((s) => s.activeSessionId);
  const createSession = useProjects((s) => s.createSession);
  const { picks, setPick, cancel, enqueue, modelOptions, setModelOptions } = useChat();
  const busy = useChat((s) => (activeSessionId ? isStreaming(s.conversations[activeSessionId]) : false));
  const setWorkspaceTab = useUi((s) => s.setWorkspaceTab);
  const openTrustPrompt = useUi((s) => s.openTrustPrompt);
  const [mode, setModeState] = useState<RequestMode>(defaultMode);
  // Always recorded, so turning "remember" on later starts from the latest choice.
  const setMode = (m: RequestMode) => {
    setModeState(m);
    setSetting("lastMode", m);
  };
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const worktree = useSettings((s) => s.sessionWorktree);
  const [starting, setStarting] = useState(false);
  const [triagePreview, setTriagePreview] = useState<TriageResult | null>(null);
  const pipeline = useExecution((s) => (activeSessionId ? s.pipelines[activeSessionId] : undefined));
  const execActive = !!pipeline && pipeline.tasks.length > 0 && pipeline.status !== "DONE";
  const [execMode, setExecMode] = useState<ExecMode>("supplement");
  const [targetTask, setTargetTask] = useState("");
  const [confirm, setConfirm] = useState<ExecConfirm | null>(null);
  const [execBusy, setExecBusy] = useState(false);
  const stateOf = (id: string) => pipeline?.taskStates[id] ?? "PENDING";
  const targetTasks = pipeline?.tasks.filter((x) => !FINISHED_STATES.includes(stateOf(x.id))) ?? [];
  const selectedTask = targetTasks.find((x) => x.id === targetTask) ?? targetTasks[0];
  const showsTriagePreview = !execActive && (mode === "auto" || mode === "fixed");

  useEffect(() => {
    if (!showsTriagePreview || !text.trim()) {
      setTriagePreview(null);
      return;
    }
    const timer = setTimeout(() => {
      triageRequest(text, project?.path)
        .then((res) => setTriagePreview(res))
        .catch(() => {
          const fallback = triage(text);
          setTriagePreview({
            route: fallback === "plan" ? "PLAN" : "DIRECT",
            confidence: 0.7,
            detectedDomains: [],
            matchedRules: [],
            estimatedFiles: [],
            scorePlan: fallback === "plan" ? 2 : 0,
            scoreDirect: fallback === "direct" ? 2 : 0,
            reason: "",
          });
        });
    }, 150);
    return () => clearTimeout(timer);
  }, [showsTriagePreview, text, project?.path]);

  const autoPredictedMode: TurnMode = triagePreview
    ? (triagePreview.route === "PLAN" ? "plan" : "direct")
    : triage(text);

  const groupsFor = useMemo(
    () => ({
      plan: isDemoProject ? selectableGroups(builtinDebugCatalog(), {}, {}, "plan") : selectableGroups(
        providers,
        enabled,
        trust,
        "plan",
        blockedModels,
        banningLevel,
        modelProviderPreferences,
        modelChannelAccess,
      ),
      direct: isDemoProject ? selectableGroups(builtinDebugCatalog(), {}, {}, "direct") : selectableGroups(
        providers,
        enabled,
        trust,
        "direct",
        blockedModels,
        banningLevel,
        modelProviderPreferences,
        modelChannelAccess,
      ),
    }),
    [isDemoProject, providers, enabled, trust, blockedModels, banningLevel, modelProviderPreferences, modelChannelAccess],
  );
  const recommended = useMemo(
    () => ({
      plan: recommend(groupsFor.plan, "plan", modelProviderPreferences, modelChannelAccess),
      direct: recommend(groupsFor.direct, "direct", modelProviderPreferences, modelChannelAccess),
      // Single-model defaults to a capable direct pick (broader catalog than plan-only).
      fixed: recommend(groupsFor.direct, "direct", modelProviderPreferences, modelChannelAccess),
    }),
    [groupsFor, modelProviderPreferences, modelChannelAccess],
  );
  // A manual pick only sticks while its provider and model are still selectable in that mode.
  const valid = (m: TurnMode | "fixed", ref: ModelRef | null) => {
    const groups = m === "fixed" ? groupsFor.direct : groupsFor[m];
    return ref && groups.some((g) => g.provider.id === ref.provider && g.models.some((x) => x.key === ref.model))
      ? ref
      : null;
  };
  const pickFor = (m: TurnMode | "fixed") => valid(m, picks[m]) ?? recommended[m];

  const submitExec = async (prompt: string) => {
    if (!activeSessionId || !pipeline) return;
    setError(null);
    if (execMode === "ask") {
      setText("");
      setWorkspaceTab("chat");
      if (!(await askAboutPipeline(activeSessionId, prompt))) setError(t("workspace.composer.noModels"));
      return;
    }
    if (execMode === "replan") {
      const live = pipeline.tasks.filter((x) => LIVE_STATES.includes(stateOf(x.id))).map((x) => `${x.id} · ${x.title}`);
      setConfirm({ kind: "replan", text: prompt, live });
      return;
    }
    const task = selectedTask;
    if (!task) {
      setError(t("workspace.composer.exec.noTarget"));
      return;
    }
    setConfirm({ kind: "supplement", taskId: task.id, title: task.title, live: LIVE_STATES.includes(stateOf(task.id)), text: prompt });
  };

  const runConfirm = async () => {
    const c = confirm;
    if (!c || !activeSessionId) return;
    setConfirm(null);
    setExecBusy(true);
    setText("");
    setWorkspaceTab("chat");
    const say = (msg: string) => useChat.getState().postSystemMessage(activeSessionId, msg);
    try {
      if (c.kind === "supplement") {
        const r = await supplementTask(activeSessionId, c.taskId, c.text);
        say(
          r === "continued"
            ? t("workspace.composer.exec.sent.continued", { taskId: c.taskId })
            : r === "updated"
              ? t("workspace.composer.exec.sent.updated", { taskId: c.taskId })
              : t("workspace.composer.exec.sent.finished", { taskId: c.taskId }),
        );
      } else {
        await interruptExecution(activeSessionId);
        say(t("workspace.composer.exec.sent.replanInterrupted"));
        const result = await replanSession(activeSessionId, c.text);
        if ("error" in result) {
          say(t("workspaceMsg.replanError", { error: result.error }));
        } else {
          notifyReplanReady({ sessionId: activeSessionId, ...result });
        }
      }
    } finally {
      setExecBusy(false);
    }
  };

  const submit = async () => {
    const prompt = text.trim();
    if (!prompt || !project || starting || execBusy) return;
    if (project.trusted !== true) {
      setError(t("settings.cli.trust.blocked"));
      openTrustPrompt();
      return;
    }
    // While a turn runs, new messages line up behind it.
    if (activeSessionId && busy) {
      enqueue(activeSessionId, prompt, mode);
      setText("");
      setError(null);
      return;
    }

    // While a plan executes, the composer addresses the execution, not a new request.
    if (execActive) {
      await submitExec(prompt);
      return;
    }

    // F16.e: while a pipeline is active, triage append → task / replan / previous-or-fresh direct.
    if (activeSessionId) {
      const pipeline = useExecution.getState().pipelines[activeSessionId];
      if (pipeline && pipeline.status !== "DONE") {
        const routed = await routePipelineAppend(activeSessionId, prompt);
        if (routed?.action === "handled") {
          setText("");
          setError(null);
          setWorkspaceTab("chat");
          return;
        }
        if (routed?.action === "direct") {
          setError(null);
          setWorkspaceTab("chat");
          setText("");
          const prefer = routed.preferPrevious
            ? getLastDirectPrefer(activeSessionId)
            : null;
          // Keep single-model lock; otherwise force a direct route for plan/append follow-ups.
          const appendMode: RequestMode =
            mode === "fixed" ? "fixed" : prefer || mode === "plan" ? "direct" : mode;
          const result = await dispatchAsync(activeSessionId, prompt, appendMode, prefer);
          if (result === "noModel") setError(t("workspace.composer.noModels"));
          return;
        }
      }
    }

    // F16.d: preview route for empty-state UX; actual pick uses dispatchAsync / assignDirectTask.
    let chosenRoute: TurnMode | null = null;
    if (mode === "auto" || mode === "fixed") {
      try {
        const res = await triageRequest(prompt, project.path);
        chosenRoute = res.route === "PLAN" ? "plan" : "direct";
      } catch {
        chosenRoute = triage(prompt);
      }
    }

    if (!planTurn(prompt, mode, null, chosenRoute, isDemoProject ? activeSessionId ?? "tokage-demo-success-en-preview" : undefined)) {
      setError(t("workspace.composer.noModels"));
      return;
    }
    setError(null);
    let sessionId = activeSessionId;
    if (!sessionId) {
      const title = prompt.split("\n")[0].slice(0, 60);
      let wt;
      if (worktree) {
        // Branch names are short; uniqueness comes from the time-based suffix.
        setStarting(true);
        try {
          wt = await createSessionWorktree(project.path, `s-${Date.now().toString(36)}`);
        } catch (e) {
          setError(t("workspace.git.worktreeFailed", { error: String(e) }));
          return;
        } finally {
          setStarting(false);
        }
      }
      sessionId = createSession(project.id, title, wt);
    }
    setWorkspaceTab("chat");
    setText("");
    const result = await dispatchAsync(sessionId, prompt, mode);
    if (result === "noModel") setError(t("workspace.composer.noModels"));
  };

  const pickerMode: TurnMode | "fixed" | null = execActive
    ? execMode === "supplement"
      ? null
      : "plan"
    : mode === "auto"
      ? null
      : mode === "fixed"
        ? "fixed"
        : mode;
  const effectivePickerMode: TurnMode | "fixed" = pickerMode ?? autoPredictedMode;
  const groups = effectivePickerMode === "fixed" ? groupsFor.direct : groupsFor[effectivePickerMode];
  // Resolve the stored assignment against the full catalog, even if its CLI is
  // temporarily unavailable. Supplementing a task keeps its assigned model.
  const assignedSpec = selectedTask?.assigned_model;
  const assignedPick = locateSpec(providers.map((provider) => ({ provider, models: provider.modelGroups })), assignedSpec);
  const assigned = assignedSpec ? parseSpec(assignedSpec) : null;
  const assignedProvider = assignedPick?.ref.provider ?? assigned?.provider;
  const assignedDetails = assignedPick ? optionSummary(
    assignedPick.model,
    optionsOf(assignedPick.model, assignedPick.variant, selectedTask?.model_params),
    (axis, v) => axis === "effort"
      ? t(`settings.cli.efforts.${v}` as "settings.cli.efforts.low", { defaultValue: v })
      : axis === "tier"
        ? t(`workspace.composer.picker.tiers.${v}` as "workspace.composer.picker.tiers.balance", { defaultValue: v })
        : v === "default" ? t("workspace.composer.picker.contextDefault") : v.toUpperCase(),
    { thinking: t("workspace.composer.picker.thinking"), fast: t("workspace.composer.picker.fast") },
  ) : [];
  // Direct-mode limits that apply to what this composer would use (docs/11 §4.2).
  const directNotes =
    execActive || mode === "plan"
      ? []
      : [
          (mode === "auto" || mode === "fixed"
            ? groupsFor.direct.some((g) => g.provider.id === "CODEX_CLI")
            : pickFor("direct")?.provider === "CODEX_CLI") &&
            t("workspace.composer.directLimit.CODEX_READ_ONLY"),
          groupsFor.plan.some((g) => directBlocked(g.provider)) && t("workspace.composer.directLimit.cursorExcluded"),
        ].filter((x): x is string => !!x);

  return (
    <div className="mx-auto w-full max-w-3xl px-3 pb-4 sm:px-6 sm:pb-6">
      {error && (
        <p role="alert" className="mb-2 text-center text-xs text-warning">
          {error}
        </p>
      )}
      {activeSessionId && <MessageQueue sessionId={activeSessionId} onRestore={(v) => setText((cur) => (cur.trim() ? `${cur.trimEnd()}\n\n${v}` : v))} />}
      <div className="rounded-2xl border border-border bg-elevated shadow-sm focus-within:border-ring">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void submit();
            }
          }}
          rows={3}
          aria-label={busy ? t("workspace.composer.placeholderBusy") : t("workspace.composer.placeholder")}
          aria-describedby="composer-mode-desc"
          placeholder={busy ? t("workspace.composer.placeholderBusy") : t("workspace.composer.placeholder")}
          className="block w-full resize-none bg-transparent px-4 pt-3 text-[14px] text-fg placeholder:text-subtle focus:outline-none"
        />
        <div className="flex items-center justify-between gap-2 px-3 pb-3">
          {execActive ? (
            <div className="flex min-w-0 items-center gap-1.5">
              <div role="radiogroup" aria-label={t("workspace.composer.mode")} className="flex shrink-0 gap-0.5 rounded-lg bg-hover p-0.5">
                {EXEC_MODES.map((m) => (
                  <button
                    key={m}
                    type="button"
                    role="radio"
                    aria-checked={execMode === m}
                    onClick={() => setExecMode(m)}
                    className={cn(
                      "h-6 rounded-md px-2.5 text-xs",
                      execMode === m ? "bg-elevated text-fg shadow-sm" : "text-muted hover:text-fg",
                    )}
                  >
                    {t(`workspace.composer.exec.modes.${m}`)}
                  </button>
                ))}
              </div>
              {execMode === "supplement" && (
                <select
                  aria-label={t("workspace.composer.exec.target")}
                  value={selectedTask?.id ?? ""}
                  disabled={targetTasks.length === 0}
                  onChange={(e) => setTargetTask(e.target.value)}
                  className="h-6 min-w-0 max-w-[10rem] truncate rounded-md bg-hover px-1.5 text-xs text-fg"
                >
                  {targetTasks.map((x) => (
                    <option key={x.id} value={x.id}>
                      {x.id} · {x.title}
                    </option>
                  ))}
                </select>
              )}
            </div>
          ) : (
          <div role="radiogroup" aria-label={t("workspace.composer.mode")} className="flex shrink-0 gap-0.5 rounded-lg bg-hover p-0.5">
            {MODES.map((m) => (
              <button
                key={m}
                type="button"
                role="radio"
                aria-checked={mode === m}
                onClick={() => setMode(m)}
                className={cn(
                  "h-6 rounded-md px-2.5 text-xs",
                  mode === m ? "bg-elevated text-fg shadow-sm" : "text-muted hover:text-fg",
                )}
              >
                {t(`workspace.composer.modes.${m}`)}
              </button>
            ))}
          </div>
          )}
          <div className="flex min-w-0 items-center gap-1">
            {execActive && execMode === "supplement" ? (
              <span
                className="inline-flex h-7 min-w-0 max-w-72 items-center rounded-md px-2 text-xs text-muted"
                title={t("workspace.composer.exec.lockedModelHint")}
              >
                {assignedProvider ? (
                  <ModelLabel
                    provider={assignedProvider}
                    providerName={providers.find((p) => p.id === assignedProvider)?.name}
                    modelName={assignedPick?.model.name ?? assigned!.model}
                    details={assignedDetails}
                  />
                ) : (
                  <span className="truncate">{assigned?.model ?? t("workspace.composer.exec.lockedModel")}</span>
                )}
              </span>
            ) : (
            <ModelPicker
              label={
                pickerMode
                  ? t(`workspace.composer.picker.label.${pickerMode}`)
                  : `${t("workspace.composer.picker.label.auto")} · ${
                      (triagePreview?.route ?? (autoPredictedMode === "plan" ? "PLAN" : "DIRECT")) === "DIRECT"
                        ? t("workspace.composer.triagePreviewDirect")
                        : t("workspace.composer.triagePreviewPlan")
                    }`
              }
              groups={groups}
              disabled={pickerMode === null}
              value={pickerMode ? pickFor(pickerMode) : recommended[autoPredictedMode]}
              recommended={
                effectivePickerMode === "fixed" ? recommended.fixed : recommended[effectivePickerMode]
              }
              overridden={
                pickerMode
                  ? !!valid(pickerMode, picks[pickerMode]) &&
                    !sameRef(
                      picks[pickerMode],
                      pickerMode === "fixed" ? recommended.fixed : recommended[pickerMode],
                    )
                  : false
              }
              onChange={(ref) => {
                if (!pickerMode) return;
                const rec = pickerMode === "fixed" ? recommended.fixed : recommended[pickerMode];
                setPick(pickerMode, sameRef(ref, rec) ? null : ref);
              }}
              onReset={() => pickerMode && setPick(pickerMode, null)}
              options={modelOptions}
              onOptionsChange={setModelOptions}
            />
            )}
            {busy && (
              <button
                type="button"
                aria-label={t("workspace.composer.stop")}
                title={t("workspace.composer.stop")}
                onClick={() => activeSessionId && void cancel(activeSessionId)}
                className="inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-fg text-bg"
              >
                <Square className="size-3 fill-current" />
              </button>
            )}
            {(!busy || text.trim()) && (
              <button
                type="button"
                aria-label={busy ? t("workspace.composer.queue") : t("workspace.composer.send")}
                title={busy ? t("workspace.composer.queueHint") : t("workspace.composer.hint")}
                disabled={!text.trim() || starting}
                onClick={() => void submit()}
                className="inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-accent text-accent-fg disabled:opacity-40"
              >
                {busy ? <ListPlus className="size-4" /> : <ArrowUp className="size-4" />}
              </button>
            )}
          </div>
        </div>
        <div id="composer-mode-desc" className="border-t border-border px-4 py-2 text-xs leading-relaxed text-muted">
          <p>{execActive ? t(`workspace.composer.exec.desc.${execMode}`) : t(`workspace.composer.modeDesc.${mode}`)}</p>
          {!execActive && mode === "fixed" && (
            <p className="mt-1.5 text-warning">{t("workspace.composer.fixedWarning")}</p>
          )}
          {showsTriagePreview && triagePreview && text.trim() && (
            <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
              <span className="inline-flex items-center rounded-md bg-accent/15 px-2 py-0.5 font-medium text-accent">
                {t("workspace.composer.triageRoute", {
                  route:
                    triagePreview.route === "PLAN"
                      ? t("workspace.composer.triagePreviewPlan")
                      : t("workspace.composer.triagePreviewDirect"),
                })}
              </span>
              {triagePreview.detectedDomains.length > 0 && (
                <span className="text-[11px] text-muted">
                  ({triagePreview.detectedDomains.join(", ")})
                </span>
              )}
              <span className="text-[11px] text-subtle">
                {t("workspace.composer.triageConfidence", {
                  confidence: Math.round(triagePreview.confidence * 100),
                })}
              </span>
              {triagePreview.reason && (
                <span className="max-w-full truncate text-[11px] text-muted" title={triagePreview.reason}>
                  · {triagePreview.reason}
                </span>
              )}
            </div>
          )}
          {directNotes.map((n) => (
            <span key={n} className="mt-1 block text-warning">
              {n}
            </span>
          ))}
        </div>
      </div>
      <Modal
        open={!!confirm}
        onClose={() => setConfirm(null)}
        title={confirm?.kind === "replan" ? t("workspace.composer.exec.replanTitle") : t("workspace.composer.exec.supplementTitle")}
        className="max-w-md"
      >
        {confirm && (
          <div className="space-y-3 p-5 text-[13px] text-fg">
            {confirm.kind === "supplement" ? (
              <p>
                {confirm.live
                  ? t("workspace.composer.exec.supplementLive", { taskId: confirm.taskId, title: confirm.title })
                  : t("workspace.composer.exec.supplementIdle", { taskId: confirm.taskId, title: confirm.title })}
              </p>
            ) : (
              <>
                <p>{t("workspace.composer.exec.replanBody")}</p>
                {confirm.live.length > 0 && (
                  <ul className="list-disc space-y-0.5 pl-5 text-muted">
                    {confirm.live.map((x) => (
                      <li key={x}>{x}</li>
                    ))}
                  </ul>
                )}
              </>
            )}
            <p className="rounded-md bg-hover px-3 py-2 text-xs text-muted">{confirm.text}</p>
            <div className="flex justify-end gap-2">
              <Button variant="ghost" size="sm" onClick={() => setConfirm(null)}>
                {t("workspace.composer.exec.cancel")}
              </Button>
              <Button variant="primary" size="sm" onClick={() => void runConfirm()}>
                {confirm.kind === "replan" ? t("workspace.composer.exec.replanConfirm") : t("workspace.composer.exec.supplementConfirm")}
              </Button>
            </div>
          </div>
        )}
      </Modal>
      <ComposerContext worktree={worktree} onWorktree={(v) => setSetting("sessionWorktree", v)} />
    </div>
  );
}
