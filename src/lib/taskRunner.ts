import type {
  ChatEvent,
  ModelParams,
  DispatchIneligibleReason,
  FailureEvidence,
  PipelineGateResult,
  Plan,
  PlanContract,
  ProviderId,
  ProviderInfo,
  PlanBudgetCheckResult,
  RepairDecision,
  RepairStep,
  TaskItem,
  TaskReport,
  UpstreamArtifact,
} from "./ipc";
import {
  assembleTaskBrief,
  cancelChatTurn,
  checkPlanBudgetOverrun,
  commitTaskWorktree,
  createSessionWorktree,
  cancelGateCommand,
  decideRepairAction,
  executeGateCommand,
  exportIntegrationPatch,
  generateDeliverySummary,
  generateImplementationFixTask,
  getTaskReports,
  gitRevParse,
  gitStagedDiffNames,
  listProviderHealth,
  loadPlan,
  mergeIntegrationToCheckout,
  mergeTaskWorktree,
  onChatEnded,
  onChatEvent,
  parseOrGenerateTaskReport,
  pickDispatchableTasks,
  pathIsDir,
  planCostSummary,
  prepareIntegrationWorktree,
  pruneProjectWorktrees,
  reassignTask,
  reassignUnstartedTasks,
  recordGateRun,
  recordTaskOutcome,
  recordTaskReport,
  removeTaskWorktree,
  resolveApproval,
  settlePlanSavings,
  startChatTurn,
  terminalResumeCommand,
  upsertAttempt,
  validateTaskBriefHeader,
  validateUserMessageIntegrity,
  validateWindowContinuity,
  validateWriteScope,
  type ChatEndedEvent,
  type GateCommandResult,
  type ProviderMeltedEvent,
  type ProviderRecoveredEvent,
  type TaskOutcomeResult,
  type UserDecision,
  type WindowConfig,
} from "./ipc";
import type { TaskState } from "./ipc";
import { currentAvailableModels, optionAxes, runnable, selectableGroups } from "./models";
import { locateSpec, parseSpec } from "./modelSpec";
import { reconcileModel } from "./modelReconcile";
import type { ModelChannelAccess } from "./modelAccess";
import { useChat } from "../stores/chat";
import { useExecution, type TaskApproval } from "../stores/execution";
import { useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";
import { pushUiEvent } from "./uiEvents";
import { findReusableWindow } from "./windowReuse";
import { lm, lmRef, lmText, renderLogMsg, type LmRef } from "./logMsg";
import { isNoneToken, REPORT_NONE } from "./reportTokens";

const deliveryInFlight = new Set<string>();

/** F12 / F13: domain → ChatRequest.role */
function roleFromDomain(domain: string): "EXECUTOR" | "TESTER" | "REVIEWER" {
  if (domain === "testing") return "TESTER";
  if (domain === "review") return "REVIEWER";
  return "EXECUTOR";
}

function windowConfigFor(
  model: string,
  role: "EXECUTOR" | "TESTER" | "REVIEWER",
  provider: string,
): WindowConfig {
  return { model, role, startupArgs: [provider] };
}

/** R9: compare to stored fingerprint, or record on first sight. */
async function enforceWindowContinuity(
  sessionId: string,
  taskId: string,
  config: WindowConfig,
): Promise<void> {
  const initial =
    useExecution.getState().pipelines[sessionId]?.taskWindowConfigs?.[taskId];
  if (initial) {
    await validateWindowContinuity(initial, config);
  } else {
    useExecution.getState().setTaskWindowConfig(sessionId, taskId, config);
  }
}

/** Localizable budget message (the backend text is an English fallback only). */
function budgetMessageRef(check: PlanBudgetCheckResult, planBudgetUsd?: number | null): LmRef {
  const fmt = (n: number | undefined) => (n ?? 0).toFixed(2);
  if (check.isRedlineExceeded) {
    return lmRef("taskLog.budget.msgRedline", { actual: fmt(check.actualCostUsd), budget: fmt(planBudgetUsd ?? 0) });
  }
  if (check.isRepairOverrun) {
    return lmRef("taskLog.budget.msgRepair", { repair: fmt(check.repairCostUsd), est: fmt(check.estCostUsd) });
  }
  if (check.is50PctOverrun) {
    return lmRef("taskLog.budget.msgOver50", { actual: fmt(check.actualCostUsd), est: fmt(check.estCostUsd) });
  }
  return lmRef("taskLog.budget.msgOk");
}

const REPAIR_STEP_LABEL: Record<RepairStep, LmRef> = {
  INITIAL: lmRef("taskLog.step.initial"),
  IN_WINDOW_REPAIR: lmRef("taskLog.step.inWindowRepair"),
  REASSIGNED: lmRef("taskLog.step.reassigned"),
  BLOCKED: lmRef("taskLog.step.blocked"),
};

function repairStepOrdinal(step: RepairStep): number {
  if (step === "IN_WINDOW_REPAIR") return 1;
  if (step === "REASSIGNED") return 2;
  if (step === "BLOCKED") return 3;
  return 0;
}

function asRepairStep(raw: string | undefined | null): RepairStep {
  if (
    raw === "IN_WINDOW_REPAIR" ||
    raw === "REASSIGNED" ||
    raw === "BLOCKED" ||
    raw === "INITIAL"
  ) {
    return raw;
  }
  return "INITIAL";
}

/** F13.b: derive MainSession.state from pipeline occupancy. */
export function syncMainSessionState(sessionId: string): void {
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (!pipeline) return;
  const states = pipeline.tasks.map((t) => pipeline.taskStates[t.id] ?? "PENDING");
  const active = states.some(
    (s) => s === "RUNNING" || s === "VERIFYING" || s === "REPAIRING",
  );
  const blocked = states.some((s) => s === "BLOCKED");
  const terminal = (s: TaskState) =>
    s === "DONE" || s === "SKIPPED" || s === "CANCELLED";
  const allTerminal = pipeline.tasks.length > 0 && states.every(terminal);

  let next: "review" | "running" | "done" | "blocked" = "running";
  if (pipeline.status === "DONE" || allTerminal) next = "done";
  else if (blocked && !active) next = "blocked";
  else if (active || pipeline.status === "RUNNING" || pipeline.status === "PAUSED") {
    next = "running";
  }

  useProjects.getState().setSessionState(sessionId, next);
}

async function recordAttemptStart(opts: {
  turnId: string;
  sessionId: string;
  task: TaskItem;
  provider: string;
  model: string;
  workdir: string;
  predecessorId?: string | null;
}): Promise<void> {
  const pipe = useExecution.getState().pipelines[opts.sessionId];
  try {
    await upsertAttempt({
      id: opts.turnId,
      mainSessionId: opts.sessionId,
      taskId: pipe?.taskDbIds?.[opts.task.id] ?? null,
      role: roleFromDomain(opts.task.domain),
      providerName: opts.provider,
      modelId: opts.model,
      predecessorId: opts.predecessorId ?? null,
      execMode: "CLI",
      workdir: opts.workdir,
      status: "RUNNING",
      baselineTokens: 0,
      contextTokens: 0,
      tokenSource: "unknown",
      turnCount: 1,
      touchedFilesJson: "[]",
    });
  } catch (err) {
    console.warn("[TaskRunner] upsert_attempt (start) failed:", err);
  }
}

async function recordAttemptEnd(
  turnId: string,
  status: "DONE" | "FAILED" | "CANCELLED" | "TIMEOUT",
): Promise<void> {
  try {
    // Upsert only status/ended fields by re-sending minimal row; Rust ON CONFLICT updates status.
    await upsertAttempt({
      id: turnId,
      mainSessionId: "",
      taskId: null,
      role: "EXECUTOR",
      providerName: "",
      modelId: "",
      execMode: "CLI",
      workdir: "",
      status,
      baselineTokens: 0,
      contextTokens: 0,
      tokenSource: "unknown",
      turnCount: 1,
      touchedFilesJson: "[]",
      endedAt: new Date().toISOString(),
    });
  } catch {
    // non-fatal
  }
}

function parseDefectEntries(defects: string): Array<{ implTaskId: string; description: string }> {
  const out: Array<{ implTaskId: string; description: string }> = [];
  for (const line of defects.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || isNoneToken(trimmed)) continue;
    const m = trimmed.match(/^-\s*([A-Za-z0-9_-]+)\s*:\s*(.+)$/);
    if (m) out.push({ implTaskId: m[1], description: m[2].trim() });
  }
  return out;
}

/** F13.e: inject `<id>-fix` tasks from tester defect notes. */
async function injectDefectFixTasks(
  sessionId: string,
  testTask: TaskItem,
  defects: string,
  evidence: string,
): Promise<void> {
  const entries = parseDefectEntries(defects);
  if (entries.length === 0) return;
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline) return;

  let tasks = [...pipeline.tasks];
  for (const entry of entries) {
    const impl = tasks.find((t) => t.id === entry.implTaskId);
    if (!impl) continue;
    const fixId = `${entry.implTaskId}-fix`;
    if (tasks.some((t) => t.id === fixId)) continue;
    try {
      const fix = await generateImplementationFixTask(
        testTask,
        impl,
        entry.description,
        evidence,
      );
      tasks = [...tasks, fix];
      executionStore.appendTaskLog(
        sessionId,
        testTask.id,
        lm("taskLog.defect.fixCreated", { id: fix.id, implTaskId: entry.implTaskId, p2: entry.description.slice(0, 80) }),
      );
    } catch (err) {
      console.warn("[TaskRunner] generateImplementationFixTask failed:", err);
    }
  }
  if (tasks.length === pipeline.tasks.length) return;
  executionStore.updatePipelineTasks(sessionId, tasks);
  for (const t of tasks) {
    if (!pipeline.taskStates[t.id]) {
      executionStore.updateTaskState(
        sessionId,
        t.id,
        "PENDING",
        lm("taskLog.defect.waitingUpstream"),
      );
    }
  }
}

/** F13.f: pause queue when budget redlines fire. */
/** F14.g: map repair step → task_outcomes.result for the completing attempt. */
function outcomeResultForCompletion(
  repairStep: string | undefined | null,
): TaskOutcomeResult {
  if (repairStep === "IN_WINDOW_REPAIR") return "FIXED_IN_WINDOW";
  if (repairStep === "REASSIGNED") return "FIRST_PASS"; // new model first pass after reassign
  return "FIRST_PASS";
}

async function recordOutcomeForTask(
  sessionId: string,
  task: TaskItem,
  result: TaskOutcomeResult,
  turnId: string,
  costUsd?: number | null,
): Promise<void> {
  const pipeline = useExecution.getState().pipelines[sessionId];
  const provider =
    pipeline?.taskProviders?.[task.id] ||
    pipeline?.assignments?.find((a) => a.taskId === task.id)?.provider ||
    "unknown";
  const modelSpec = task.assigned_model || "unknown";
  const modelId = modelSpec.includes(":")
    ? modelSpec.slice(modelSpec.indexOf(":") + 1)
    : modelSpec;
  const assignment = pipeline?.assignments?.find((a) => a.taskId === task.id);
  try {
    await recordTaskOutcome(
      {
        taskId: task.id,
        modelId,
        providerName: provider,
        domain: task.domain,
        difficulty: task.difficulty,
        size: task.size,
        result,
        pPredicted: assignment?.pSuccess ?? 0.8,
        estCostUsd: assignment?.estCostUsd ?? null,
        actualCostUsd: costUsd ?? null,
        turns: 1,
      },
      turnId,
    );
  } catch (err) {
    console.warn("[TaskRunner] record_task_outcome failed:", err);
  }
}

async function maybeCheckPlanBudget(sessionId: string): Promise<void> {
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (!pipeline?.planDbId) return;
  const { planBudgetUsd, pauseOnOverrun } = useSettings.getState();
  const est =
    pipeline.assignments?.reduce((sum, a) => sum + (a.estCostUsd ?? 0), 0) ?? 0;
  try {
    const summary = await planCostSummary(pipeline.planDbId);
    const check = await checkPlanBudgetOverrun(
      est,
      summary.total,
      planBudgetUsd,
      summary.repair,
      pauseOnOverrun,
    );
    if (check.shouldPauseQueue && pipeline.status === "RUNNING") {
      useExecution.getState().pauseExecution(sessionId);
      const logTaskId = pipeline.tasks[0]?.id ?? "_";
      useExecution.getState().appendTaskLog(
        sessionId,
        logTaskId,
        lm("taskLog.budget.redline", { message: budgetMessageRef(check, planBudgetUsd) }),
      );
    } else if (
      (check.is50PctOverrun || check.isRepairOverrun || check.isRedlineExceeded) &&
      !check.shouldPauseQueue
    ) {
      const logTaskId = pipeline.tasks[0]?.id ?? "_";
      useExecution.getState().appendTaskLog(
        sessionId,
        logTaskId,
        lm("taskLog.budget.hint", { message: budgetMessageRef(check, planBudgetUsd) }),
      );
    }
  } catch (err) {
    console.warn("[TaskRunner] budget check failed:", err);
  }
}

function planFromPipeline(sessionId: string): Plan | null {
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (!pipeline) return null;
  return {
    tasks: pipeline.tasks,
    contracts: pipeline.contracts ?? [],
    spec: { raw: "", goal: "", inScope: [], outOfScope: [], decisions: [] },
    rawMarkdown: "",
  };
}

/** F15.f: provider is hard-melted (QUOTA_HARD / AUTH) — skip in-window repair. */
async function isProviderHardMelted(provider: string): Promise<{
  melted: boolean;
  until: string | null;
  reason: string | null;
}> {
  try {
    const rows = await listProviderHealth();
    const row = rows.find((r) => r.providerName === provider);
    if (!row || row.status !== "MELTED") {
      return { melted: false, until: null, reason: null };
    }
    const reason = row.meltReason;
    if (reason === "QUOTA_HARD" || reason === "AUTH") {
      return { melted: true, until: row.meltedUntil, reason };
    }
    // HEALTH / QUOTA_SOFT: still melted for allocation, but running-task path differs.
    if (reason === "HEALTH") {
      return { melted: true, until: row.meltedUntil, reason };
    }
    return { melted: false, until: row.meltedUntil, reason };
  } catch {
    return { melted: false, until: null, reason: null };
  }
}

/**
 * F15.f: reassign unstarted tasks away from a melted provider; update assignments snapshot.
 */
export async function onProviderMeltedEvent(e: ProviderMeltedEvent): Promise<void> {
  const pipelines = useExecution.getState().pipelines;
  for (const [sessionId, pipeline] of Object.entries(pipelines)) {
    if (!pipeline || pipeline.status === "DONE") continue;
    const plan = planFromPipeline(sessionId);
    if (!plan || !pipeline.assignments?.length) continue;

    // Single-model mode: never swap to another model on melt; pause unstarted tasks instead.
    const locked = useChat.getState().conversations[sessionId]?.lockedModel;
    if (locked) {
      const executionStore = useExecution.getState();
      for (const t of pipeline.tasks) {
        const st = pipeline.taskStates[t.id];
        if (st !== "PENDING" && st !== "READY") continue;
        const assigned = t.assigned_model ?? "";
        if (!assigned.startsWith(`${e.provider}:`)) continue;
        executionStore.updateTaskState(
          sessionId,
          t.id,
          "PAUSED_QUOTA",
          lm("taskLog.quota.lockedPaused", { provider: locked.provider, model: locked.model, p2: e.provider, reason: e.reason }),
        );
      }
      syncMainSessionState(sessionId);
      continue;
    }

    const completedOrRunning = pipeline.tasks
      .filter((t) => {
        const st = pipeline.taskStates[t.id];
        return (
          st === "RUNNING" ||
          st === "VERIFYING" ||
          st === "REPAIRING" ||
          st === "DONE" ||
          st === "SKIPPED" ||
          st === "CANCELLED" ||
          st === "BLOCKED" ||
          st === "INTERRUPTED"
        );
      })
      .map((t) => t.id);

    try {
      const result = await reassignUnstartedTasks(
        plan,
        pipeline.assignments,
        completedOrRunning,
        [e.provider],
        currentAvailableModels(),
      );
      const executionStore = useExecution.getState();
      for (const a of result.assignments) {
        const prev = pipeline.assignments.find((x) => x.taskId === a.taskId);
        if (prev && prev.provider === e.provider && a.provider !== e.provider) {
          const modelSpec = `${a.provider}:${a.cliModelArg}`;
          executionStore.updateTaskAssignedModel(sessionId, a.taskId, modelSpec);
          executionStore.appendTaskLog(
            sessionId,
            a.taskId,
            lm("taskLog.reassign.providerTripped", { provider: e.provider, reason: e.reason, modelSpec }),
          );
        }
      }
      // Refresh assignments snapshot if the store supports replacing the array.
      if (result.assignments.length > 0) {
        useExecution.setState((s) => {
          const p = s.pipelines[sessionId];
          if (!p) return s;
          return {
            pipelines: {
              ...s.pipelines,
              [sessionId]: { ...p, assignments: result.assignments },
            },
          };
        });
      }
    } catch (err) {
      console.warn("[TaskRunner] reassign unstarted after melt failed:", err);
    }
  }
}

/**
 * F15.f: resume PAUSED_QUOTA tasks when their provider recovers.
 */
export async function onProviderRecoveredEvent(e: ProviderRecoveredEvent): Promise<void> {
  const pipelines = useExecution.getState().pipelines;
  for (const [sessionId, pipeline] of Object.entries(pipelines)) {
    if (!pipeline) continue;
    for (const t of pipeline.tasks) {
      if (pipeline.taskStates[t.id] !== "PAUSED_QUOTA") continue;
      const provider =
        pipeline.taskProviders?.[t.id] ||
        pipeline.assignments?.find((a) => a.taskId === t.id)?.provider ||
        (t.assigned_model?.includes(":") ? t.assigned_model.split(":")[0] : null);
      if (provider && provider !== e.provider) continue;
      useExecution.getState().updateTaskState(
        sessionId,
        t.id,
        "READY",
        lm("taskLog.recover.providerRecovered", { provider: e.provider }),
      );
    }
    void dispatchReadyTasks(sessionId).catch(() => {});
  }
}

/**
 * F13.c: gate / R6 failure → decideRepairAction ladder.
 * Returns whether a repair turn was started (caller should not BLOCKED).
 */
export async function applyRepairLadder(
  ctx: RunningSubAgentContext,
  evidence: FailureEvidence,
  forceMelted?: boolean,
): Promise<"repairing" | "blocked" | "paused_quota"> {
  const executionStore = useExecution.getState();
  const { sessionId, taskId, task } = ctx;
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline) return "blocked";

  executionStore.recordTaskFailureEvidence(sessionId, taskId, {
    failedCommand: evidence.failedCommand ?? null,
    outputTail: evidence.outputTail,
    outOfScopeFiles: evidence.outOfScopeFiles,
    previousChangedFiles: evidence.previousChangedFiles,
    workdir: ctx.worktreeCwd,
  });

  const currentStep = asRepairStep(pipeline.repairSteps?.[taskId]);
  const provider =
    pipeline.taskProviders?.[taskId] ||
    pipeline.assignments?.find((a) => a.taskId === taskId)?.provider ||
    "unknown";
  const model = task.assigned_model || "unknown";
  const modelId = model.includes(":") ? model.slice(model.indexOf(":") + 1) : model;

  const meltInfo = forceMelted
    ? { melted: true, until: null as string | null, reason: null as string | null }
    : await isProviderHardMelted(provider);

  let nextStep: RepairStep;
  let decision: RepairDecision;
  try {
    [nextStep, decision] = await decideRepairAction(
      task,
      currentStep,
      ctx.turnId,
      modelId,
      provider,
      evidence,
      meltInfo.melted,
    );
  } catch (err) {
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "BLOCKED",
      lm("taskLog.blocked.repairDecisionFailed", { err }),
    );
    syncMainSessionState(sessionId);
    return "blocked";
  }

  executionStore.setRepairStep(sessionId, taskId, nextStep);
  const n = repairStepOrdinal(nextStep);
  executionStore.appendTaskLog(
    sessionId,
    taskId,
    lm("taskLog.ladder.step", { n, nextStep: REPAIR_STEP_LABEL[nextStep], currentStep: REPAIR_STEP_LABEL[currentStep] }),
  );

  // Degrade in-window repair without thread id → reassign
  let effective = decision;
  if ("tryInWindowRepair" in decision) {
    const threadId = pipeline.threadIds?.[taskId];
    if (!threadId) {
      executionStore.appendTaskLog(
        sessionId,
        taskId,
        lm("taskLog.ladder.noThread"),
      );
      effective = {
        tryReassign: {
          excludeModels: [modelId],
          evidence: decision.tryInWindowRepair.evidence,
        },
      };
      executionStore.setRepairStep(sessionId, taskId, "REASSIGNED");
      nextStep = "REASSIGNED";
    }
  }

  if ("tryInWindowRepair" in effective) {
    const threadId = pipeline.threadIds![taskId];
    const evidenceMd = effective.tryInWindowRepair.evidence;
    activeSubAgents.delete(ctx.turnId);
    void recordAttemptEnd(ctx.turnId, "FAILED");

    executionStore.updateTaskState(
      sessionId,
      taskId,
      "REPAIRING",
      lm("taskLog.inWindow.preparing"),
    );

    const turnId = `subagent-${sessionId}-${taskId}-${Date.now().toString(36)}`;
    const claimed = executionStore.claimTaskRunning(
      sessionId,
      taskId,
      turnId,
      lm("taskLog.inWindow.launching", { turnId }),
    );
    if (!claimed) {
      executionStore.updateTaskState(
        sessionId,
        taskId,
        "BLOCKED",
        lm("taskLog.blocked.inWindowClaimFailed"),
      );
      syncMainSessionState(sessionId);
      return "blocked";
    }

    void ensureSubAgentEventListener();
    registerRunningSubAgent(turnId, {
      sessionId,
      taskId,
      task,
      worktreeCwd: ctx.worktreeCwd,
      turnId,
      rawOutput: "",
      changedFiles: [...ctx.changedFiles],
      toolCalls: [],
      startTime: Date.now(),
    });

    const project = useProjects.getState().projects.find((p) =>
      p.sessions.some((s) => s.id === sessionId),
    );
    const { providers } = useProviders.getState();
    const {
      providerEnabled,
      providerTrust,
      blockedModels,
      modelBanningLevel,
      modelProviderPreferences,
      modelChannelAccess,
    } = useSettings.getState();
    const resolved = resolveTaskProviderAndModel(
      task.assigned_model,
      providers,
      providerEnabled,
      providerTrust,
      blockedModels,
      modelBanningLevel,
      modelProviderPreferences,
      modelChannelAccess,
      task.model_params,
    );
    if (!resolved) {
      failClaimedSubAgent(sessionId, taskId, turnId, lm("taskLog.error.inWindowNoModel"));
      syncMainSessionState(sessionId);
      return "blocked";
    }

    const trustedList: ProviderId[] =
      project?.trusted === true
        ? (Object.entries(useSettings.getState().providerTrust)
            .filter(([, v]) => v)
            .map(([k]) => k as ProviderId))
        : [resolved.provider];

    const prompt = `## Repair Instructions (same window)\nFix the problem using the failure evidence below, then output the task report again.\n\n${evidenceMd}`;
    const role = roleFromDomain(task.domain);

    try {
      await enforceWindowContinuity(
        sessionId,
        taskId,
        windowConfigFor(resolved.model, role, resolved.provider),
      );
      await recordAttemptStart({
        turnId,
        sessionId,
        task,
        provider: resolved.provider,
        model: resolved.model,
        workdir: ctx.worktreeCwd,
        predecessorId: ctx.turnId,
      });
      await startChatTurn({
        turnId,
        cwd: ctx.worktreeCwd,
        prompt,
        mode: "direct",
        provider: resolved.provider,
        model: resolved.model,
        effort: resolved.effort,
        fast: resolved.fast,
        tier: resolved.tier,
        history: [],
        resume: threadId,
        fork: false,
        trusted: trustedList,
        sessionId,
        projectId: project?.id ?? null,
        sessionAllowed: pipeline.taskAllowed?.[taskId] ?? [],
        role,
        writeScope: task.write_scope,
        timeoutTier: "AGENT",
        idleTimeoutSecs: Math.round((useSettings.getState().agentIdleTimeoutMin ?? 30) * 60),
        hardTimeoutSecs: Math.round((useSettings.getState().agentHardTimeoutMin ?? 120) * 60),
        planId: pipeline.planDbId ?? null,
        taskId: pipeline.taskDbIds?.[taskId] ?? null,
        domain: task.domain,
        phase: "REPAIR",
        attemptId: turnId,
      });
    } catch (err) {
      failClaimedSubAgent(sessionId, taskId, turnId, lm("taskLog.error.inWindowLaunchFailed", { err }));
      syncMainSessionState(sessionId);
      return "blocked";
    }
    syncMainSessionState(sessionId);
    return "repairing";
  }

  if ("tryReassign" in effective) {
    const { excludeModels, evidence: evidenceMd } = effective.tryReassign;

    // Single-model mode: do not switch models; escalate to BLOCKED for user takeover.
    const locked = useChat.getState().conversations[sessionId]?.lockedModel;
    if (locked) {
      activeSubAgents.delete(ctx.turnId);
      void recordAttemptEnd(ctx.turnId, "FAILED");
      executionStore.setRepairStep(sessionId, taskId, "BLOCKED");
      executionStore.updateTaskState(
        sessionId,
        taskId,
        "BLOCKED",
        lm("taskLog.blocked.lockedSkipReassign", { provider: locked.provider, model: locked.model, evidenceMd }),
      );
      syncMainSessionState(sessionId);
      return "blocked";
    }

    activeSubAgents.delete(ctx.turnId);
    void recordAttemptEnd(ctx.turnId, "FAILED");
    // F14.g: record REASSIGNED against the previous model before switching
    void recordOutcomeForTask(sessionId, task, "REASSIGNED", ctx.turnId, null);

    executionStore.updateTaskState(
      sessionId,
      taskId,
      "REPAIRING",
      lm("taskLog.reassign.excluding", { p0: excludeModels.join(", ") || lmRef("taskLog.none") }),
    );

    const plan = planFromPipeline(sessionId);
    if (!plan) {
      executionStore.updateTaskState(sessionId, taskId, "BLOCKED", lm("taskLog.blocked.reassignNoPlan"));
      syncMainSessionState(sessionId);
      return "blocked";
    }

    let assignment;
    try {
      assignment = await reassignTask(plan, taskId, excludeModels, currentAvailableModels());
    } catch (err) {
      // F15.f: no alternate provider → PAUSED_QUOTA (await recover) instead of BLOCKED.
      if (meltInfo.melted) {
        const until = meltInfo.until ? lmRef("taskLog.quota.resumeAt", { until: meltInfo.until }) : "";
        executionStore.updateTaskState(
          sessionId,
          taskId,
          "PAUSED_QUOTA",
          lm("taskLog.quota.noAlternate", { provider, reason: meltInfo.reason ?? "unknown", until, err }),
        );
        syncMainSessionState(sessionId);
        return "paused_quota";
      }
      executionStore.updateTaskState(
        sessionId,
        taskId,
        "BLOCKED",
        lm("taskLog.blocked.candidatesExhausted", { err }),
      );
      syncMainSessionState(sessionId);
      return "blocked";
    }

    const modelSpec = `${assignment.provider}:${assignment.cliModelArg}`;
    executionStore.updateTaskAssignedModel(sessionId, taskId, modelSpec);
    // New window: startChatTurn uses resume: null (dispatchSubAgentTask); stale threadId is ok.

    const pipe2 = useExecution.getState().pipelines[sessionId];
    // A new model starts clean: the old model's effort/fast/tier flags don't carry over.
    const freshTask = {
      ...task,
      assigned_model: modelSpec,
      model_params: null,
    };
    const contracts = pipe2?.contracts ?? [];
    const upstream = Object.values(pipe2?.upstreamArtifacts ?? {});

    executionStore.appendTaskLog(
      sessionId,
      taskId,
      lm("taskLog.reassign.newModel", { modelSpec, p1: assignment.pSuccess.toFixed(2) }),
    );

    try {
      await dispatchSubAgentTask({
        sessionId,
        task: freshTask,
        contracts,
        upstreamArtifacts: upstream,
        failureEvidence: evidenceMd,
        baseCwd: ctx.worktreeCwd,
        useWorktree: false, // reuse existing cwd / worktree
      });
    } catch (err) {
      const st = pipelineTaskState(sessionId, taskId);
      if (st === "REPAIRING") {
        executionStore.updateTaskState(
          sessionId,
          taskId,
          "BLOCKED",
          lm("taskLog.blocked.startAfterReassignFailed", { err }),
        );
      }
      syncMainSessionState(sessionId);
      return "blocked";
    }
    syncMainSessionState(sessionId);
    return "repairing";
  }

  if ("markBlocked" in effective) {
    activeSubAgents.delete(ctx.turnId);
    void recordAttemptEnd(ctx.turnId, "FAILED");
    void recordOutcomeForTask(sessionId, task, "BLOCKED", ctx.turnId, null);
    const { terminalResumeCommand, reason } = effective.markBlocked;
    let cmd = terminalResumeCommand;
    try {
      const got = await getTaskTerminalResumeCommand(sessionId, taskId);
      if (got.command) cmd = got.command;
    } catch {
      /* keep ladder command */
    }
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "BLOCKED",
      lm("taskLog.blocked.manualTakeover", { reason, cmd }),
    );
    syncMainSessionState(sessionId);
    return "blocked";
  }

  activeSubAgents.delete(ctx.turnId);
  executionStore.updateTaskState(sessionId, taskId, "BLOCKED", lm("taskLog.blocked.unknownRepairDecision"));
  syncMainSessionState(sessionId);
  return "blocked";
}

export interface RunningSubAgentContext {
  sessionId: string;
  taskId: string;
  task: TaskItem;
  worktreeCwd: string;
  turnId: string;
  rawOutput: string;
  changedFiles: string[];
  toolCalls: Array<{ id: string; tool: string; title: string }>;
  startTime: number;
  /** F6.b: terminal chat events seen for this turn. */
  sawDone?: boolean;
  sawError?: boolean;
  /**
   * F6.c: approval decision recorded while the CLI turn was still running;
   * resume after chat-ended.
   */
  pendingResume?: { approvalId: string; decision: UserDecision; command: string };
}

const activeSubAgents = new Map<string, RunningSubAgentContext>();

export function getRunningSubAgent(turnId: string): RunningSubAgentContext | undefined {
  return activeSubAgents.get(turnId);
}

export function registerRunningSubAgent(turnId: string, ctx: RunningSubAgentContext) {
  activeSubAgents.set(turnId, ctx);
}

export function unregisterRunningSubAgent(turnId: string) {
  activeSubAgents.delete(turnId);
}

/** Tasks that (transitively) depend on `rootId`. */
export function transitiveDownstreamTaskIds(tasks: TaskItem[], rootId: string): string[] {
  const out = new Set<string>();
  let grew = true;
  while (grew) {
    grew = false;
    for (const t of tasks) {
      if (t.id === rootId || out.has(t.id)) continue;
      if (t.depends_on.some((d) => d === rootId || out.has(d))) {
        out.add(t.id);
        grew = true;
      }
    }
  }
  return [...out];
}

function pipelineTaskState(sessionId: string, taskId: string): TaskState | undefined {
  return useExecution.getState().pipelines[sessionId]?.taskStates[taskId];
}

/** Expected states while a turn is still allowed to drive further transitions. */
function isCancellablePipelineState(state: TaskState | undefined): boolean {
  return state === "RUNNING" || state === "VERIFYING";
}

/** Map Rust `Command timed out (idle|hard timeout exceeded)` to a BLOCKED reason (F3.e). */
export function formatAgentTimeoutBlockReason(message: string): string | null {
  const idle = /idle timeout exceeded/i.test(message);
  const hard = /hard timeout exceeded/i.test(message);
  if (!idle && !hard) return null;
  const settings = useSettings.getState();
  const minutes = idle
    ? (settings.agentIdleTimeoutMin ?? 30)
    : (settings.agentHardTimeoutMin ?? 120);
  const kind = idle ? lmRef("taskLog.timeout.idle") : lmRef("taskLog.timeout.hard");
  return lm("taskLog.blocked.timeout", { kind, minutes });
}

/**
 * F7.d / S5: resume an INTERRUPTED task after app restart.
 * With thread id → resume CLI session; without → fresh dispatch with task brief.
 * Never auto-launched by `dispatchReadyTasks`.
 */
export async function continueTask(sessionId: string, taskId: string): Promise<void> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline || pipeline.unrecoverable) return;

  const state = pipeline.taskStates[taskId];
  if (state !== "INTERRUPTED") return;

  const task = pipeline.tasks.find((t) => t.id === taskId);
  if (!task) return;

  const baseCwd = pipeline.baseCwd || pipeline.cwd || ".";
  let taskCwd = pipeline.taskCwds?.[taskId] || baseCwd;
  // CLI sessions are keyed by their working directory; a missing task cwd (e.g. a cleaned-up
  // worktree) means the old session cannot be resumed, so fall back to a fresh dispatch.
  let threadId = pipeline.threadIds?.[taskId];
  if (!taskCwd.trim()) {
    taskCwd = baseCwd;
    executionStore.appendTaskLog(
      sessionId,
      taskId,
      lm("taskLog.resume.cwdMissing", { baseCwd }),
    );
  } else if (taskCwd !== baseCwd && !(await pathIsDir(taskCwd))) {
    taskCwd = baseCwd;
    threadId = undefined;
    executionStore.appendTaskLog(
      sessionId,
      taskId,
      lm("taskLog.resume.cwdMissing", { baseCwd }),
    );
  } else if (taskCwd !== baseCwd && !pipeline.taskCwds?.[taskId]) {
    executionStore.appendTaskLog(
      sessionId,
      taskId,
      lm("taskLog.resume.useSessionCwd", { taskCwd }),
    );
  }

  // Mark pipeline running again when user continues.
  if (pipeline.status === "INTERRUPTED") {
    executionStore.resumeExecution(sessionId);
  }

  if (!threadId) {
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "READY",
      lm("taskLog.resume.noThreadId"),
    );
    await dispatchReadyTasks(sessionId, { only: [taskId] });
    return;
  }

  // F16.e: prefer queued main-session follow-up for this task when present.
  const { takePendingTaskFollowUp } = await import("./appendTriage");
  const followUp = takePendingTaskFollowUp(sessionId, taskId);
  const prompt =
    followUp?.trim() ||
    "Continue the unfinished task; if the previous round already finished it, just output the task report.";

  executionStore.updateTaskState(
    sessionId,
    taskId,
    "READY",
    lm("taskLog.resume.preparing", { threadId }),
  );

  const { providers } = useProviders.getState();
  const {
    providerEnabled,
    providerTrust,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
  } = useSettings.getState();
  const resolved = resolveTaskProviderAndModel(
    task.assigned_model,
    providers,
    providerEnabled,
    providerTrust,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
    task.model_params,
  );
  if (!resolved) {
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "BLOCKED",
      lm("taskLog.error.resumeNoModel", { assigned_model: task.assigned_model ?? "null" }),
    );
    return;
  }

  const turnId = `subagent-${sessionId}-${taskId}-${Date.now().toString(36)}`;
  const claimed = executionStore.claimTaskRunning(
    sessionId,
    taskId,
    turnId,
    lm("taskLog.resume.launching", { turnId }),
  );
  if (!claimed) {
    executionStore.appendTaskLog(sessionId, taskId, lm("taskLog.resume.claimFailed"));
    return;
  }

  void ensureSubAgentEventListener();
  registerRunningSubAgent(turnId, {
    sessionId,
    taskId,
    task,
    worktreeCwd: taskCwd,
    turnId,
    rawOutput: "",
    changedFiles: [],
    toolCalls: [],
    startTime: Date.now(),
  });
  executionStore.setTaskCwd(sessionId, taskId, taskCwd);
  executionStore.setTaskProvider(sessionId, taskId, resolved.provider);

  const project = useProjects.getState().projects.find((p) =>
    p.sessions.some((s) => s.id === sessionId),
  );
  const trustedList: ProviderId[] =
    project?.trusted === true
      ? (Object.entries(useSettings.getState().providerTrust)
          .filter(([, v]) => v)
          .map(([k]) => k as ProviderId))
      : [resolved.provider];

  const sessionAllowed = pipeline.taskAllowed?.[taskId] ?? [];
  const role = roleFromDomain(task.domain);

  try {
    if (followUp?.trim()) {
      await validateUserMessageIntegrity(followUp.trim(), prompt);
    }
    await enforceWindowContinuity(
      sessionId,
      taskId,
      windowConfigFor(resolved.model, role, resolved.provider),
    );
    await startChatTurn({
      turnId,
      cwd: taskCwd,
      prompt,
      mode: "direct",
      provider: resolved.provider,
      model: resolved.model,
      effort: resolved.effort,
      fast: resolved.fast,
      tier: resolved.tier,
      history: [],
      resume: threadId,
      fork: false,
      trusted: trustedList,
      sessionId,
      projectId: project?.id ?? null,
      sessionAllowed,
      role,
      writeScope: task.write_scope,
      timeoutTier: "AGENT",
      idleTimeoutSecs: Math.round((useSettings.getState().agentIdleTimeoutMin ?? 30) * 60),
      hardTimeoutSecs: Math.round((useSettings.getState().agentHardTimeoutMin ?? 120) * 60),
      planId: pipeline.planDbId ?? null,
      taskId: pipeline.taskDbIds?.[taskId] ?? null,
      domain: task.domain,
      phase: "EXECUTION",
      attemptId: turnId,
    });
  } catch (err) {
    failClaimedSubAgent(sessionId, taskId, turnId, lm("taskLog.error.resumeFailed", { err }));
  }
}

/**
 * Cancel a pipeline task (S2 / F3.d): stop the CLI if RUNNING, mark CANCELLED,
 * skip not-yet-started transitive downstream, and drop the sub-agent context.
 */
export async function cancelTask(sessionId: string, taskId: string): Promise<void> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline) return;

  const state = pipeline.taskStates[taskId];
  if (!isCancellablePipelineState(state)) return;

  const turnId = pipeline.taskTurnIds?.[taskId];
  if (state === "RUNNING" && turnId) {
    await cancelChatTurn(turnId);
  }
  // F5.e: VERIFYING gates are cancelled via GateRuns, not chat turns.
  if (state === "VERIFYING") {
    await cancelGateCommand(`gate:${sessionId}:${taskId}`);
  }

  executionStore.updateTaskState(
    sessionId,
    taskId,
    "CANCELLED",
    lm("taskLog.cancel.byUser", { taskId }),
  );
  if (turnId) unregisterRunningSubAgent(turnId);

  const downstream = transitiveDownstreamTaskIds(pipeline.tasks, taskId);
  for (const depId of downstream) {
    const st = pipelineTaskState(sessionId, depId);
    if (st === "PENDING" || st === "READY" || st === "WAITING_UPSTREAM") {
      executionStore.updateTaskState(
        sessionId,
        depId,
        "SKIPPED",
        lm("taskLog.skip.upstreamCancelled", { taskId }),
      );
    }
  }
}

/**
 * Stops one in-flight task (CLI turn or gate) but keeps its CLI thread id and worktree:
 * it becomes INTERRUPTED, exactly as after an app restart, so `continueTask` can resume it.
 * Resolves once the CLI process group has exited.
 */
async function interruptTask(sessionId: string, taskId: string): Promise<void> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline) return;
  const state = pipeline.taskStates[taskId];
  if (state !== "RUNNING" && state !== "VERIFYING" && state !== "REPAIRING") return;
  const turnId = pipeline.taskTurnIds?.[taskId];
  // Mark first: late events from the dying CLI are ignored for non-RUNNING tasks.
  executionStore.updateTaskState(
    sessionId,
    taskId,
    "INTERRUPTED",
    lm("taskLog.interrupt.byUser", { taskId }),
  );
  if (turnId) unregisterRunningSubAgent(turnId);
  try {
    if (state === "VERIFYING") await cancelGateCommand(`gate:${sessionId}:${taskId}`);
    else if (turnId) await cancelChatTurn(turnId);
  } catch (err) {
    console.warn(`[interrupt] stopping ${taskId} failed:`, err);
  }
}

/** Stops every in-flight task of a pipeline and blocks new dispatch until the user continues. */
export async function interruptExecution(sessionId: string): Promise<void> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline || pipeline.status === "DONE") return;

  // Block dispatch first so finishing tasks cannot start new ones while we stop the rest.
  executionStore.interruptPipeline(sessionId);
  await Promise.all(pipeline.tasks.map((t) => interruptTask(sessionId, t.id)));
}

export type SupplementResult = "continued" | "updated" | "finished" | "noTask";

/**
 * Sends the user's new instruction to one task. A running task is interrupted first (the new
 * instruction may contradict what it is doing) and resumed in its own CLI session with the
 * instruction as the next message. A task that has not started gets it appended to its goal.
 */
export async function supplementTask(
  sessionId: string,
  taskId: string,
  text: string,
): Promise<SupplementResult> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  const task = pipeline?.tasks.find((t) => t.id === taskId);
  if (!pipeline || !task) return "noTask";
  const state = pipeline.taskStates[taskId] ?? "PENDING";
  if (state === "DONE" || state === "CANCELLED" || state === "SKIPPED") return "finished";

  const instruction = text.trim();
  executionStore.appendTaskLog(
    sessionId,
    taskId,
    lm("taskLog.user.followUp", { p0: instruction.slice(0, 200), p1: instruction.length > 200 ? "…" : "" }),
  );

  if (state === "RUNNING" || state === "VERIFYING" || state === "REPAIRING" || state === "INTERRUPTED") {
    const { queueTaskFollowUp } = await import("./appendTriage");
    // Queue before stopping so `continueTask` finds it; drop it again if nothing resumes.
    queueTaskFollowUp(sessionId, taskId, instruction);
    await interruptTask(sessionId, taskId);
    await continueTask(sessionId, taskId);
    return "continued";
  }

  // Not started (or waiting): fold the instruction into the task definition it will run from.
  executionStore.updatePipelineTasks(
    sessionId,
    pipeline.tasks.map((t) =>
      t.id === taskId ? { ...t, goal: `${t.goal}\n\nUser update:\n${instruction}` } : t,
    ),
  );
  return "updated";
}

/** Continues every INTERRUPTED task of a pipeline (each resumes its own CLI session). */
export async function continueInterrupted(sessionId: string): Promise<void> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline) return;
  const ids = pipeline.tasks
    .filter((t) => pipeline.taskStates[t.id] === "INTERRUPTED")
    .map((t) => t.id);
  if (ids.length === 0) {
    if (pipeline.status === "INTERRUPTED") {
      executionStore.resumeExecution(sessionId);
      await dispatchReadyTasks(sessionId);
    }
    return;
  }
  for (const id of ids) {
    await continueTask(sessionId, id);
  }
}

let subAgentListenerCleanup: (() => void) | null = null;
let chatEndedListenerCleanup: (() => void) | null = null;

export async function ensureSubAgentEventListener() {
  if (!subAgentListenerCleanup) {
    subAgentListenerCleanup = await onChatEvent(handleSubAgentChatEvent);
  }
  if (!chatEndedListenerCleanup) {
    chatEndedListenerCleanup = await onChatEnded(handleSubAgentChatEnded);
  }
}

export function resetSubAgentEventListener() {
  if (subAgentListenerCleanup) {
    subAgentListenerCleanup();
    subAgentListenerCleanup = null;
  }
  if (chatEndedListenerCleanup) {
    chatEndedListenerCleanup();
    chatEndedListenerCleanup = null;
  }
  activeSubAgents.clear();
}

function pendingApprovalsForTask(sessionId: string, taskId: string): TaskApproval[] {
  const list = useExecution.getState().pipelines[sessionId]?.taskApprovals?.[taskId] ?? [];
  return list.filter((a) => !a.resolved);
}

/**
 * F6.b: turn ended without done/error. Pending approvals → BLOCKED waiting for user;
 * otherwise BLOCKED for abnormal CLI exit. Cancelled ends are handled by cancelTask.
 */
export async function handleSubAgentChatEnded(e: ChatEndedEvent): Promise<void> {
  if (e.cancelled) return;

  const ctx = activeSubAgents.get(e.turnId);
  if (!ctx) return;

  const st = pipelineTaskState(ctx.sessionId, ctx.taskId);
  if (st !== "RUNNING") {
    activeSubAgents.delete(e.turnId);
    return;
  }

  const pending = pendingApprovalsForTask(ctx.sessionId, ctx.taskId);
  const executionStore = useExecution.getState();

  // Prefer deferred resume even if a late `done` also arrived this turn.
  if (ctx.pendingResume) {
    const { decision, command } = ctx.pendingResume;
    activeSubAgents.delete(e.turnId);
    await resumePipelineTaskAfterApproval(ctx.sessionId, ctx.taskId, decision, command);
    return;
  }

  if (ctx.sawDone || ctx.sawError) {
    // done/error already drove completion or BLOCKED; drop ctx if still present.
    activeSubAgents.delete(e.turnId);
    return;
  }

  if (pending.length > 0) {
    const cmds = pending.map((a) => a.command).join("; ");
    executionStore.updateTaskState(
      ctx.sessionId,
      ctx.taskId,
      "BLOCKED",
      lm("taskLog.approval.waiting", { cmds }),
    );
    activeSubAgents.delete(e.turnId);
    return;
  }

  executionStore.updateTaskState(
    ctx.sessionId,
    ctx.taskId,
    "BLOCKED",
    lm("taskLog.exec.cliExitedNoTerminal"),
  );
  activeSubAgents.delete(e.turnId);
}

/**
 * F6.c: record approval decision, update taskAllowed, and resume or defer until chat-ended.
 */
export async function resolvePipelineTaskApproval(
  sessionId: string,
  taskId: string,
  approvalId: string,
  decision: UserDecision,
): Promise<void> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline) return;

  const approval = (pipeline.taskApprovals?.[taskId] ?? []).find((a) => a.id === approvalId);
  if (!approval || approval.resolved) return;

  const project = useProjects.getState().projects.find((p) =>
    p.sessions.some((s) => s.id === sessionId),
  );

  try {
    await resolveApproval({
      id: approvalId,
      sessionId,
      turnId: approval.turnId,
      toolId: approval.toolId,
      command: approval.command,
      decision,
      pattern: approval.command,
      projectId: project?.id,
    });
  } catch (err) {
    console.error("Failed to record pipeline approval:", err);
  }

  executionStore.resolveTaskApproval(sessionId, taskId, approvalId, decision);
  if (decision !== "deny") {
    executionStore.addTaskAllowedCommand(sessionId, taskId, approval.command);
  }

  const turnId = pipeline.taskTurnIds?.[taskId];
  const ctx = turnId ? activeSubAgents.get(turnId) : undefined;
  const stillRunning = pipelineTaskState(sessionId, taskId) === "RUNNING" && !!ctx;

  if (stillRunning && ctx) {
    ctx.pendingResume = { approvalId, decision, command: approval.command };
    executionStore.appendTaskLog(
      sessionId,
      taskId,
      lm("taskLog.approval.recorded", { decision }),
    );
    return;
  }

  await resumePipelineTaskAfterApproval(sessionId, taskId, decision, approval.command);
}

export async function resumePipelineTaskAfterApproval(
  sessionId: string,
  taskId: string,
  decision: UserDecision,
  command: string,
): Promise<void> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline) return;
  const task = pipeline.tasks.find((t) => t.id === taskId);
  if (!task) return;

  const threadId = pipeline.threadIds?.[taskId];
  if (!threadId) {
    executionStore.appendTaskLog(
      sessionId,
      taskId,
      lm("taskLog.approval.noThreadId"),
    );
    return;
  }

  const prompt =
    decision === "deny"
      ? `User denied command: ${command}. Please continue without running this command or find an alternative approach.`
      : `User approved command: ${command}. Please execute it now.`;

  executionStore.updateTaskState(sessionId, taskId, "READY", lm("taskLog.approval.preparing", { decision }));

  const sessionAllowed = pipeline.taskAllowed?.[taskId] ?? [];
  const worktreeCwd =
    pipeline.taskCwds?.[taskId] || pipeline.baseCwd || pipeline.cwd || ".";

  // Re-dispatch through TaskRunner path so AGENT timeouts / write_scope apply.
  // Mark READY first so claimTaskRunning succeeds; then start with resume.
  const { providers } = useProviders.getState();
  const {
    providerEnabled,
    providerTrust,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
  } = useSettings.getState();
  const resolved = resolveTaskProviderAndModel(
    task.assigned_model,
    providers,
    providerEnabled,
    providerTrust,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
    task.model_params,
  );
  if (!resolved) {
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "BLOCKED",
      lm("taskLog.error.approvalNoModel"),
    );
    return;
  }

  const turnId = `subagent-${sessionId}-${taskId}-${Date.now().toString(36)}`;
  const claimed = executionStore.claimTaskRunning(
    sessionId,
    taskId,
    turnId,
    lm("taskLog.approval.launching", { turnId }),
  );
  if (!claimed) {
    executionStore.appendTaskLog(sessionId, taskId, lm("taskLog.approval.claimFailed"));
    return;
  }

  void ensureSubAgentEventListener();
  registerRunningSubAgent(turnId, {
    sessionId,
    taskId,
    task,
    worktreeCwd,
    turnId,
    rawOutput: "",
    changedFiles: [],
    toolCalls: [],
    startTime: Date.now(),
  });

  const project = useProjects.getState().projects.find((p) =>
    p.sessions.some((s) => s.id === sessionId),
  );
  const trustedList: ProviderId[] =
    project?.trusted === true
      ? (Object.entries(useSettings.getState().providerTrust)
          .filter(([, v]) => v)
          .map(([k]) => k as ProviderId))
      : [resolved.provider];

  const role = roleFromDomain(task.domain);
  try {
    await enforceWindowContinuity(
      sessionId,
      taskId,
      windowConfigFor(resolved.model, role, resolved.provider),
    );
    await startChatTurn({
      turnId,
      cwd: worktreeCwd,
      prompt,
      mode: "direct",
      provider: resolved.provider,
      model: resolved.model,
      effort: resolved.effort,
      fast: resolved.fast,
      tier: resolved.tier,
      history: [],
      resume: threadId,
      fork: false,
      trusted: trustedList,
      sessionId,
      projectId: project?.id ?? null,
      sessionAllowed,
      role,
      writeScope: task.write_scope,
      timeoutTier: "AGENT",
      idleTimeoutSecs: Math.round((useSettings.getState().agentIdleTimeoutMin ?? 30) * 60),
      hardTimeoutSecs: Math.round((useSettings.getState().agentHardTimeoutMin ?? 120) * 60),
      planId: pipeline.planDbId ?? null,
      taskId: pipeline.taskDbIds?.[taskId] ?? null,
      domain: task.domain,
      phase: "EXECUTION",
      attemptId: turnId,
    });
  } catch (err) {
    failClaimedSubAgent(sessionId, taskId, turnId, lm("taskLog.error.approvalResumeFailed", { err }));
  }
}

export interface ResolvedModel {
  provider: ProviderId;
  model: string;
  effort: string | null;
  fast: boolean;
  tier: string | null;
  fallbackUsed?: boolean;
}

/**
 * Maps an assigned model string (e.g. "CLAUDE_CODE:sonnet") to a concrete runnable
 * ProviderId and CLI model parameters. Returns null when the CLI/model is missing,
 * disabled, untrusted, or not runnable — no silent fallback (F1.f).
 *
 * Matching never uses bare substrings: a variant id wins exactly, then the model key/name,
 * then an id the spec only extends by mode suffixes (see `findInCatalog`). `params` are the
 * flags the id can't carry (effort / fast / tier) and override what the variant implies.
 */
export function resolveTaskProviderAndModel(
  assignedModelSpec: string | null | undefined,
  providers: ProviderInfo[],
  enabled: Partial<Record<ProviderId, boolean>> = {},
  trust: Partial<Record<ProviderId, boolean>> = {},
  blockedModels: string[] = [],
  banningLevel: "off" | "moderate" | "strict" = "off",
  modelProviderPreferences?: Record<string, { prefer?: string; never?: string[] }>,
  modelChannelAccess?: ModelChannelAccess,
  modelParams?: ModelParams | null,
  customModels?: Partial<Record<ProviderId, string[]>>,
): ResolvedModel | null {
  const spec = (assignedModelSpec || "").trim();
  if (!spec) return null;

  const groups = selectableGroups(
    providers,
    enabled,
    trust,
    "direct",
    blockedModels,
    banningLevel,
    modelProviderPreferences,
    modelChannelAccess,
    customModels,
  );
  const picked = locateSpec(groups, spec);
  if (!picked || !runnable(groups.find((g) => g.provider.id === picked.ref.provider)!.provider)) return null;

  const { variant, model } = picked;
  // Flags come from the id when it spells them, otherwise from the stored params.
  const axes = optionAxes(model);
  const effort = variant.effort ?? modelParams?.effort ?? null;
  return {
    provider: picked.ref.provider,
    model: variant.id,
    effort: effort && effort !== "default" ? effort : null,
    fast: variant.fast || (axes.fast && !!modelParams?.fast),
    tier: modelParams?.tier ?? null,
  };
}

export interface TaskRunnerOptions {
  sessionId: string;
  task: TaskItem;
  contracts?: PlanContract[];
  upstreamArtifacts?: UpstreamArtifact[];
  failureEvidence?: string | null;
  baseCwd: string;
  useWorktree?: boolean;
}

/**
 * Assembles task brief prompt for a sub-agent.
 */
export async function buildSubAgentPrompt(
  task: TaskItem,
  contracts: PlanContract[] = [],
  upstreamArtifacts: UpstreamArtifact[] = [],
  failureEvidence?: string | null,
): Promise<string> {
  return assembleTaskBrief({
    task,
    contracts,
    upstreamArtifacts,
    failureEvidence: failureEvidence ?? null,
  });
}

/**
 * TaskRunner: Orchestrates starting a sub-agent process with brief assembly,
 * model resolution, workspace isolation, Agent-tier timeouts, and state binding.
 */
function failClaimedSubAgent(
  sessionId: string,
  taskId: string,
  turnId: string,
  reason: string,
): void {
  unregisterRunningSubAgent(turnId);
  useExecution.getState().updateTaskState(sessionId, taskId, "BLOCKED", reason);
}

export async function dispatchSubAgentTask(
  options: TaskRunnerOptions,
): Promise<{ turnId: string; worktreeCwd: string; resolved: ResolvedModel }> {
  const { sessionId, task, contracts = [], upstreamArtifacts = [], failureEvidence, baseCwd, useWorktree } = options;
  const executionStore = useExecution.getState();

  // 1. Resolve Provider and Model (sync; failure before claim)
  const { providers } = useProviders.getState();
  const {
    providerEnabled,
    providerTrust,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
  } = useSettings.getState();

  const resolved = resolveTaskProviderAndModel(
    task.assigned_model,
    providers,
    providerEnabled,
    providerTrust,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
    task.model_params,
  );

  if (!resolved) {
    const reason = lm("taskLog.error.noRunnableModel", { id: task.id, assigned_model: task.assigned_model ?? "null" });
    executionStore.updateTaskState(sessionId, task.id, "BLOCKED", reason);
    throw new Error(renderLogMsg(reason));
  }

  // 2. Sync claim before any await (F1.c): READY/REPAIRING → RUNNING + register ctx
  const predecessorId =
    executionStore.pipelines[sessionId]?.taskTurnIds?.[task.id] ?? null;
  const turnId = `subagent-${sessionId}-${task.id}-${Date.now().toString(36)}`;
  const claimLog = lm("taskLog.dispatch.launching", { turnId, provider: resolved.provider, model: resolved.model });
  const claimed = executionStore.claimTaskRunning(sessionId, task.id, turnId, claimLog);
  if (!claimed) {
    throw new Error(lmText("taskLog.error.alreadyClaimed", { id: task.id }));
  }

  void ensureSubAgentEventListener();
  registerRunningSubAgent(turnId, {
    sessionId,
    taskId: task.id,
    task,
    worktreeCwd: baseCwd,
    turnId,
    rawOutput: "",
    changedFiles: [],
    toolCalls: [],
    startTime: Date.now(),
  });

  try {
    // 3. Assemble Task Brief + R10 fixed-header check before workspace / reuse.
    const prompt = await buildSubAgentPrompt(
      task,
      contracts,
      upstreamArtifacts,
      failureEvidence,
    );
    await validateTaskBriefHeader(prompt);

    const role = roleFromDomain(task.domain);
    const cfg = windowConfigFor(resolved.model, role, resolved.provider);

    // 4. Window reuse (docs/05 §4): prefer a warm DONE peer before new worktree.
    const pipeForReuse = useExecution.getState().pipelines[sessionId];
    let resume: string | null = null;
    let taskCwd = baseCwd;
    let reusedPeer: string | null = null;
    if (pipeForReuse) {
      const reusable = await findReusableWindow({
        pipeline: pipeForReuse,
        task,
        model: resolved.model,
        prompt,
        preferredCwd: useWorktree ? null : baseCwd,
      });
      if (reusable) {
        // Cross-worktree resume is unsafe; only reuse when peer cwd is shared base
        // or we are not isolating into a fresh worktree.
        const peerIsBase =
          reusable.cwd === baseCwd ||
          reusable.cwd === pipeForReuse.baseCwd ||
          reusable.cwd === pipeForReuse.cwd;
        if (!useWorktree || peerIsBase) {
          resume = reusable.threadId;
          taskCwd = reusable.cwd;
          reusedPeer = reusable.taskId;
          useExecution.getState().appendTaskLog(
            sessionId,
            task.id,
            lm("taskLog.window.reuse", { taskId: reusable.taskId, reason: reusable.decision.reason, p2: (reusable.decision.savingPercentage * 100).toFixed(0) }),
          );
        }
      }
    }

    // 5. Isolate workspace if requested and not reusing a peer window
    if (useWorktree && !resume) {
      const existing = executionStore.pipelines[sessionId]?.taskWorktrees?.[task.id];
      if (existing?.cwd) {
        taskCwd = existing.cwd;
        const ctx = getRunningSubAgent(turnId);
        if (ctx) ctx.worktreeCwd = taskCwd;
        useExecution.getState().appendTaskLog(
          sessionId,
          task.id,
          lm("taskLog.workspace.reused", { taskCwd }),
        );
      } else {
        await ensureSessionIntegration(sessionId);
        const pipe = useExecution.getState().pipelines[sessionId];
        const baseRef = pipe?.integration?.branch ?? null;
        const attempt = executionStore.nextTaskAttempt(sessionId, task.id);
        const rawName = `${sid8(sessionId)}-${task.id}-${attempt}`;
        const wtName = rawName.replace(/[^A-Za-z0-9-]/g, "").slice(0, 40);
        const wt = await createSessionWorktree(baseCwd, wtName, baseRef);
        if (wt?.cwd) {
          taskCwd = wt.cwd;
          const ctx = getRunningSubAgent(turnId);
          if (ctx) ctx.worktreeCwd = taskCwd;
          let baseCommit = "";
          try {
            baseCommit = await gitRevParse(wt.root, "HEAD");
          } catch {
            /* non-fatal */
          }
          executionStore.setTaskWorktree(sessionId, task.id, {
            root: wt.root,
            cwd: wt.cwd,
            branch: wt.branch,
            baseCommit,
          });
          useExecution.getState().appendTaskLog(
            sessionId,
            task.id,
            lm("taskLog.workspace.new", { taskCwd, branch: wt.branch, p2: baseCommit.slice(0, 8) || "?" }),
          );
        }
      }
    } else {
      const ctx = getRunningSubAgent(turnId);
      if (ctx) ctx.worktreeCwd = taskCwd;
    }

    // F7: persist task cwd + provider for crash recovery / continue.
    executionStore.setTaskCwd(sessionId, task.id, taskCwd);
    executionStore.setTaskProvider(sessionId, task.id, resolved.provider);

    // R9: resume into a reused peer window inherits that fingerprint; else record ours.
    if (resume && reusedPeer) {
      const peerCfg =
        useExecution.getState().pipelines[sessionId]?.taskWindowConfigs?.[reusedPeer];
      if (peerCfg) {
        await validateWindowContinuity(peerCfg, cfg);
        executionStore.setTaskWindowConfig(sessionId, task.id, peerCfg);
      } else {
        executionStore.setTaskWindowConfig(sessionId, task.id, cfg);
      }
    } else {
      executionStore.setTaskWindowConfig(sessionId, task.id, cfg);
    }

    // 6. Project trust
    const project = useProjects.getState().projects.find((p) => p.sessions.some((s) => s.id === sessionId));
    const trustedList: ProviderId[] =
      project?.trusted === true
        ? (Object.entries(useSettings.getState().providerTrust)
            .filter(([, v]) => v)
            .map(([k]) => k as ProviderId))
        : [resolved.provider];

    // 7. Invoke startChatTurn (F12: plan/task/domain/phase/attempt attribution)
    const pipeMeta = useExecution.getState().pipelines[sessionId];
    const phase = failureEvidence ? "REPAIR" : "EXECUTION";
    await recordAttemptStart({
      turnId,
      sessionId,
      task,
      provider: resolved.provider,
      model: resolved.model,
      workdir: taskCwd,
      predecessorId:
        predecessorId && predecessorId !== turnId ? predecessorId : null,
    });
    await startChatTurn({
      turnId,
      cwd: taskCwd,
      prompt,
      mode: "direct",
      provider: resolved.provider,
      model: resolved.model,
      effort: resolved.effort,
      fast: resolved.fast,
      tier: resolved.tier,
      history: [],
      resume,
      fork: false,
      trusted: trustedList,
      sessionId,
      projectId: project?.id ?? null,
      role,
      writeScope: task.write_scope,
      timeoutTier: "AGENT",
      idleTimeoutSecs: Math.round((useSettings.getState().agentIdleTimeoutMin ?? 30) * 60),
      hardTimeoutSecs: Math.round((useSettings.getState().agentHardTimeoutMin ?? 120) * 60),
      planId: pipeMeta?.planDbId ?? null,
      taskId: pipeMeta?.taskDbIds?.[task.id] ?? null,
      domain: task.domain,
      phase,
      attemptId: turnId,
    });

    syncMainSessionState(sessionId);

    return {
      turnId,
      worktreeCwd: taskCwd,
      resolved,
    };
  } catch (err) {
    failClaimedSubAgent(
      sessionId,
      task.id,
      turnId,
      lm("taskLog.error.spawnFailed", { err }),
    );
    throw err;
  }
}

/**
 * Global chat-event handler routing output to the corresponding sub-agent task (m4a).
 */
export async function handleSubAgentChatEvent(e: ChatEvent): Promise<void> {
  let ctx = activeSubAgents.get(e.turnId);
  const executionStore = useExecution.getState();

  // If not in in-memory map, check if any pipeline session contains this turnId
  if (!ctx) {
    for (const session of Object.values(executionStore.pipelines)) {
      if (session.taskTurnIds) {
        for (const [taskId, turnId] of Object.entries(session.taskTurnIds)) {
          if (turnId === e.turnId) {
            const task = session.tasks.find((t) => t.id === taskId);
            if (task) {
              // F7.c: rebuild with persisted task definition and task cwd.
              ctx = {
                sessionId: session.sessionId,
                taskId,
                task,
                worktreeCwd:
                  session.taskCwds?.[taskId] ||
                  session.baseCwd ||
                  session.cwd ||
                  ".",
                turnId: e.turnId,
                rawOutput: "",
                changedFiles: [],
                toolCalls: [],
                startTime: session.taskStartedAt?.[taskId] || Date.now(),
                sawDone: false,
                sawError: false,
              };
              activeSubAgents.set(e.turnId, ctx);
              break;
            }
          }
        }
      }
      if (ctx) break;
    }
  }

  // Not a sub-agent event
  if (!ctx) return;

  const liveState = () => pipelineTaskState(ctx!.sessionId, ctx!.taskId);

  switch (e.kind) {
    case "model":
      if (!isCancellablePipelineState(liveState())) break;
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, lm("taskLog.event.modelAssigned", { name: e.name }));
      {
        // Requested vs reported: remember what an alias resolved to, flag a different model.
        const want = parseSpec(ctx.task.assigned_model ?? "");
        if (want.provider && want.model) {
          const r = reconcileModel(want.provider, want.model, e.name);
          if (r.kind === "resolved") {
            useSettings.getState().recordObservedModel(want.provider, want.model, e.name);
          } else if (r.kind === "mismatch") {
            executionStore.appendTaskLog(
              ctx.sessionId,
              ctx.taskId,
              lm("taskLog.event.modelMismatch", { requested: want.model, actual: e.name }),
            );
          }
        }
      }
      break;

    case "text":
      if (!isCancellablePipelineState(liveState())) break;
      ctx.rawOutput += e.text;
      break;

    case "thread":
      if (!isCancellablePipelineState(liveState())) break;
      executionStore.setTaskThreadId(ctx.sessionId, ctx.taskId, e.id);
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, lm("taskLog.event.session", { id: e.id }));
      break;

    case "reasoning": {
      if (!isCancellablePipelineState(liveState())) break;
      const preview = e.text.trim().split("\n")[0]?.slice(0, 120) ?? "";
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.event.thinking", { length: e.text.length, p1: preview ? lmRef("taskLog.event.thinkingPreview", { preview }) : "" }),
      );
      break;
    }

    case "quota": {
      if (!isCancellablePipelineState(liveState())) break;
      const reset = e.resetAt ? lmRef("taskLog.quota.resetAt", { resetAt: e.resetAt }) : "";
      const retry = e.retrying ? lmRef("taskLog.quota.retrying") : "";
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.event.quota", { raw: e.raw, reset, retry }),
      );
      break;
    }

    case "approvalRequest": {
      if (!isCancellablePipelineState(liveState())) break;
      const createdAtMs = Date.now();
      const approval: TaskApproval = {
        id: e.id,
        turnId: e.turnId,
        toolId: e.toolId ?? null,
        command: e.command,
        level: e.level,
        reason: e.reason,
        recoverable: e.recoverable ?? null,
        resolved: false,
        createdAtMs,
      };
      executionStore.upsertTaskApproval(ctx.sessionId, ctx.taskId, approval);
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.event.pendingApproval", { command: e.command, level: e.level, reason: e.reason }),
      );
      pushUiEvent({
        type: "approval_required",
        item: {
          approvalId: e.id,
          taskId: ctx.taskId,
          attemptId: e.turnId,
          command: e.command,
          reason: e.reason,
          createdAtMs,
        },
      });
      break;
    }

    case "toolStart": {
      if (!isCancellablePipelineState(liveState())) break;
      ctx.toolCalls.push({ id: e.id, tool: e.tool, title: e.title });
      const detail = e.detail ? ` (${e.detail})` : "";
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.event.toolCall", { tool: e.tool, title: e.title, detail }),
      );
      break;
    }

    case "toolEnd": {
      if (!isCancellablePipelineState(liveState())) break;
      const status = e.ok === false ? lmRef("taskLog.tool.failed") : lmRef("taskLog.tool.ok");
      const out = e.output ? `: ${e.output.slice(0, 150)}` : "";
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.event.toolEnd", { id: e.id, status, out }),
      );
      break;
    }

    case "fileChange": {
      if (!isCancellablePipelineState(liveState())) break;
      if (!ctx.changedFiles.includes(e.path)) {
        ctx.changedFiles.push(e.path);
      }
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.event.fileChange", { change: e.change, path: e.path, additions: e.additions, deletions: e.deletions }),
      );
      break;
    }

    case "warning":
      if (!isCancellablePipelineState(liveState())) break;
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, lm("taskLog.event.warning", { message: e.message }));
      break;

    case "error": {
      ctx.sawError = true;
      const st = liveState();
      if (!isCancellablePipelineState(st)) {
        executionStore.appendTaskLog(
          ctx.sessionId,
          ctx.taskId,
          lm("taskLog.ignore.lateError", { message: e.message }),
        );
        activeSubAgents.delete(e.turnId);
        break;
      }
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, lm("taskLog.event.error", { message: e.message }));
      const timeoutReason = formatAgentTimeoutBlockReason(e.message);
      if (timeoutReason) {
        const evidence = [
          lmText("taskLog.timeoutEvidence.lastOutput", { rawOutput: (ctx.rawOutput || "").slice(-500) || lmText("taskLog.none") }),
          lmText("taskLog.timeoutEvidence.changedFiles", { p0: ctx.changedFiles.length ? ctx.changedFiles.join(", ") : lmText("taskLog.none") }),
        ].join("\n");
        executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, lm("taskLog.timeoutEvidence.header", { evidence }));
        executionStore.updateTaskState(ctx.sessionId, ctx.taskId, "BLOCKED", timeoutReason);
        activeSubAgents.delete(e.turnId);
        break;
      }
      // F15.f: QUOTA_HARD / AUTH melt → repair ladder step 2 (skip in-window).
      const provider =
        useExecution.getState().pipelines[ctx.sessionId]?.taskProviders?.[ctx.taskId] ||
        useExecution
          .getState()
          .pipelines[ctx.sessionId]?.assignments?.find((a) => a.taskId === ctx.taskId)
          ?.provider ||
        "unknown";
      const melt = await isProviderHardMelted(provider);
      if (melt.melted && (melt.reason === "QUOTA_HARD" || melt.reason === "AUTH")) {
        await applyRepairLadder(
          ctx,
          {
            failedCommand: null,
            outputTail: e.message,
            outOfScopeFiles: [],
            previousChangedFiles: [...ctx.changedFiles],
          },
          true,
        );
        break;
      }
      executionStore.updateTaskState(
        ctx.sessionId,
        ctx.taskId,
        "BLOCKED",
        lm("taskLog.exec.processExited", { message: e.message }),
      );
      activeSubAgents.delete(e.turnId);
      break;
    }

    case "done": {
      ctx.sawDone = true;
      const st = liveState();
      if (st !== "RUNNING") {
        executionStore.appendTaskLog(
          ctx.sessionId,
          ctx.taskId,
          lm("taskLog.ignore.lateDone", { st: st ?? "unknown" }),
        );
        activeSubAgents.delete(e.turnId);
        break;
      }
      // Decision recorded while CLI still running — wait for chat-ended to resume.
      if (ctx.pendingResume) {
        break;
      }
      // F6.b: pending approvals block gate entry.
      const pending = pendingApprovalsForTask(ctx.sessionId, ctx.taskId);
      if (pending.length > 0) {
        const cmds = pending.map((a) => a.command).join("; ");
        executionStore.updateTaskState(
          ctx.sessionId,
          ctx.taskId,
          "BLOCKED",
          lm("taskLog.approval.waiting", { cmds }),
        );
        activeSubAgents.delete(e.turnId);
        break;
      }
      await processSubAgentCompletion(ctx, e);
      break;
    }
  }
}

/**
 * R6 (F5.i): files changed this turn that fall outside `task.write_scope`.
 * In `inplace` isolation, files covered by another task's write_scope whose run time
 * overlaps this task are excluded (concurrent tasks share the working tree).
 * If this task's start time is unknown, no exclusion is applied.
 */
/**
 * F11.h: worktree R6 uses `git diff --cached --name-only <baseCommit>` ∪ event files.
 * inplace keeps S8 overlap exclusion.
 */
async function findOutOfScopeFiles(ctx: RunningSubAgentContext): Promise<string[]> {
  const isolation = useSettings.getState().isolation;
  const pipeline = useExecution.getState().pipelines[ctx.sessionId];
  const wt = pipeline?.taskWorktrees?.[ctx.taskId];

  let changed = [...ctx.changedFiles];
  if (isolation === "worktree" && wt?.baseCommit && wt.root) {
    try {
      const staged = await gitStagedDiffNames(wt.root, wt.baseCommit);
      const set = new Set(changed);
      for (const f of staged) set.add(f);
      changed = [...set];
    } catch (err) {
      console.warn("[TaskRunner] worktree R6 staged diff failed:", err);
    }
  }

  if (changed.length === 0) return [];
  const scopeRes = await validateWriteScope(ctx.task.write_scope ?? [], changed);
  let outOfScope = scopeRes.outOfScopeFiles;
  if (outOfScope.length === 0 || isolation !== "inplace") {
    return outOfScope;
  }

  const myStart = pipeline?.taskStartedAt?.[ctx.taskId];
  if (!pipeline || !myStart) return outOfScope;
  const now = Date.now();
  const myEnd = pipeline.taskEndedAt?.[ctx.taskId] ?? now;

  for (const other of pipeline.tasks) {
    if (other.id === ctx.taskId || outOfScope.length === 0) continue;
    const otherStart = pipeline.taskStartedAt?.[other.id];
    if (!otherStart) continue;
    const otherEnd = pipeline.taskEndedAt?.[other.id] ?? now;
    if (!(myStart < otherEnd && otherStart < myEnd)) continue;
    const covered = await validateWriteScope(other.write_scope ?? [], outOfScope);
    if (covered.allowedFiles.length > 0) {
      const excluded = new Set(covered.allowedFiles);
      outOfScope = outOfScope.filter((f) => !excluded.has(f));
    }
  }
  return outOfScope;
}

/** Per-session merge serialization (F11). */
const mergeLocks = new Map<string, Promise<unknown>>();

async function withSessionMergeLock<T>(sessionId: string, fn: () => Promise<T>): Promise<T> {
  const prev = mergeLocks.get(sessionId) ?? Promise.resolve();
  let release!: () => void;
  const gate = new Promise<void>((r) => {
    release = r;
  });
  const chained = prev.then(() => gate);
  mergeLocks.set(sessionId, chained);
  await prev;
  try {
    return await fn();
  } finally {
    release();
    if (mergeLocks.get(sessionId) === chained) mergeLocks.delete(sessionId);
  }
}

function sid8(sessionId: string): string {
  return sessionId.replace(/[^A-Za-z0-9]/g, "").slice(0, 8) || "session";
}

/** F11.c: ensure integration branch + worktree exist for this session. */
export async function ensureSessionIntegration(sessionId: string): Promise<void> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline || pipeline.integration) return;
  if (useSettings.getState().isolation !== "worktree") return;
  const projectPath = pipeline.baseCwd || pipeline.cwd;
  if (!projectPath) return;

  const name = `${sid8(sessionId)}-integration`;
  let baseCommit: string | null = null;
  try {
    baseCommit = await gitRevParse(projectPath, "HEAD");
  } catch {
    baseCommit = null;
  }
  const wt = await createSessionWorktree(projectPath, name, null);
  executionStore.setIntegration(sessionId, {
    branch: wt.branch,
    root: wt.root,
    baseCommit,
  });
  executionStore.appendTaskLog(
    sessionId,
    pipeline.tasks[0]?.id ?? "_",
    lm("taskLog.integration.worktree", { branch: wt.branch, root: wt.root }),
  );
}

/**
 * F11.d/e: after gates pass in worktree mode — commit, merge into integration,
 * handle conflict by injecting an integration task.
 * Returns true if the caller should mark DONE (always true after this; conflict
 * still marks original DONE).
 */
async function finishWorktreeTask(
  sessionId: string,
  task: TaskItem,
): Promise<{ ok: true } | { ok: false; error: string }> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline) return { ok: false, error: "no pipeline" };
  const wt = pipeline.taskWorktrees?.[task.id];
  const integration = pipeline.integration;
  if (!wt || !integration) {
    // No worktree metadata — treat as inplace completion.
    return { ok: true };
  }

  try {
    const committed = await commitTaskWorktree(
      wt.root,
      `Tokage task ${task.id}: ${task.title}`,
    );
    executionStore.appendTaskLog(
      sessionId,
      task.id,
      committed ? lm("taskLog.commit.committed") : lm("taskLog.commit.nothing"),
    );

    const outcome = await withSessionMergeLock(sessionId, () =>
      mergeTaskWorktree(integration.root, integration.branch, wt.branch, task),
    );

    if ("success" in outcome) {
      executionStore.appendTaskLog(
        sessionId,
        task.id,
        lm("taskLog.merge.merged", { mergedBranch: outcome.success.mergedBranch }),
      );
      try {
        await removeTaskWorktree(wt.root, wt.branch, true, false);
        executionStore.setTaskWorktree(sessionId, task.id, null);
      } catch (err) {
        executionStore.appendTaskLog(
          sessionId,
          task.id,
          lm("taskLog.cleanup.worktreeFailed", { err }),
        );
      }
      return { ok: true };
    }

    // Conflict: original task DONE; inject integration task; rewrite downstream deps.
    const { conflictedFiles, integrationTask } = outcome.conflict;
    executionStore.appendTaskLog(
      sessionId,
      task.id,
      lm("taskLog.merge.conflictTask", { id: integrationTask.id, p1: conflictedFiles.join(", ") }),
    );

    const fresh = useExecution.getState().pipelines[sessionId];
    if (!fresh) return { ok: true };

    const integId = integrationTask.id;
    const rewritten = fresh.tasks.map((t) => {
      if (t.id === task.id) return t;
      if (t.depends_on.includes(task.id) && !t.depends_on.includes(integId)) {
        return { ...t, depends_on: [...t.depends_on, integId] };
      }
      return t;
    });
    // Ensure integration task is present
    if (!rewritten.some((t) => t.id === integId)) {
      rewritten.push(integrationTask);
    }
    executionStore.updatePipelineTasks(sessionId, rewritten);
    // Seed PENDING state for the new task
    executionStore.updateTaskState(
      sessionId,
      integId,
      "PENDING",
      lm("taskLog.merge.integrationWaiting", { p0: conflictedFiles.join(", ") }),
    );

    // Pre-create conflict worktree from integration + merge --no-commit
    const projectPath = fresh.baseCwd || fresh.cwd || ".";
    const attempt = executionStore.nextTaskAttempt(sessionId, integId);
    const name = `${sid8(sessionId)}-${integId.replace(/[^A-Za-z0-9-]/g, "")}-${attempt}`.slice(
      0,
      40,
    );
    try {
      const prep = await prepareIntegrationWorktree(
        projectPath,
        name,
        integration.branch,
        wt.branch,
      );
      const baseCommit = await gitRevParse(prep.root, "HEAD").catch(() => "");
      executionStore.setTaskWorktree(sessionId, integId, {
        root: prep.root,
        cwd: prep.cwd,
        branch: prep.branch,
        baseCommit,
      });
      executionStore.setTaskCwd(sessionId, integId, prep.cwd);
    } catch (err) {
      executionStore.appendTaskLog(
        sessionId,
        integId,
        lm("taskLog.integration.premergeFailed", { err }),
      );
    }

    // Clean original task worktree dir (keep branch for later merge)
    try {
      await removeTaskWorktree(wt.root, wt.branch, true, true);
      executionStore.setTaskWorktree(sessionId, task.id, null);
    } catch {
      /* best effort */
    }

    return { ok: true };
  } catch (err) {
    return { ok: false, error: String(err) };
  }
}

/** Shared gate-run context for auto-completion and terminal takeover (F10.d). */
export interface GateRunContext {
  sessionId: string;
  taskId: string;
  task: TaskItem;
  workdir: string;
  /** F13.b: attempts.id (= turnId) for gate_runs.attempt_id */
  attemptId?: string | null;
}

/**
 * Run all acceptance commands for a task already in VERIFYING (F10.d).
 * Empty command list counts as passed. `aborted` means the task left VERIFYING mid-run.
 */
export async function runGatesForTask(ctx: GateRunContext): Promise<{
  passed: boolean;
  evidence: string;
  results: PipelineGateResult[];
  aborted: boolean;
}> {
  const executionStore = useExecution.getState();
  const commands = ctx.task.acceptance?.commands ?? [];
  const results: PipelineGateResult[] = [];
  const failureOutputs: string[] = [];

  if (commands.length === 0) {
    executionStore.appendTaskLog(
      ctx.sessionId,
      ctx.taskId,
      lm("taskLog.gate.noCommands"),
    );
    return { passed: true, evidence: "", results, aborted: false };
  }

  for (const cmd of commands) {
    if (pipelineTaskState(ctx.sessionId, ctx.taskId) !== "VERIFYING") {
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.ignore.gateAborted"),
      );
      return {
        passed: false,
        evidence: failureOutputs.join("\n---\n"),
        results,
        aborted: true,
      };
    }
    executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, lm("taskLog.gate.running", { cmd }));
    try {
      const envAllow = useSettings.getState().gateEnvAllow ?? [];
      const gateRunId = `gate:${ctx.sessionId}:${ctx.taskId}`;
      const confirmed = ctx.task.acceptance?.confirmedCommands?.includes(cmd) === true;
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.gate.queued"),
      );
      const gateRes = await executeGateCommand(
        cmd,
        ctx.workdir,
        300,
        confirmed,
        envAllow,
        gateRunId,
      );
      if (pipelineTaskState(ctx.sessionId, ctx.taskId) !== "VERIFYING") {
        executionStore.appendTaskLog(
          ctx.sessionId,
          ctx.taskId,
          lm("taskLog.ignore.gateResultDropped"),
        );
        return {
          passed: false,
          evidence: failureOutputs.join("\n---\n"),
          results,
          aborted: true,
        };
      }
      const gateResult: PipelineGateResult = {
        taskId: ctx.taskId,
        command: cmd,
        passed: gateRes.passed,
        exitCode: gateRes.exitCode,
        durationMs: gateRes.durationMs,
        outputTail: gateRes.outputTail || "",
      };
      results.push(gateResult);
      executionStore.recordGateResult(ctx.sessionId, gateResult);

      // F13.b: persist each gate command to gate_runs
      const pipe = executionStore.pipelines[ctx.sessionId];
      if (pipe?.planDbId != null) {
        void recordGateRun({
          taskId: ctx.taskId,
          planId: String(pipe.planDbId),
          attemptId: ctx.attemptId ?? pipe.taskTurnIds?.[ctx.taskId] ?? null,
          command: cmd,
          exitCode: gateRes.exitCode,
          passed: gateRes.passed,
          durationMs: gateRes.durationMs,
          outputTail: gateRes.outputTail || "",
          errorMessage: gateRes.errorMessage ?? null,
        }).catch((err) => console.warn("[TaskRunner] record_gate_run failed:", err));
      }

      if (gateRes.passed) {
        executionStore.appendTaskLog(
          ctx.sessionId,
          ctx.taskId,
          lm("taskLog.gate.cmdPassed", { cmd, durationMs: gateRes.durationMs }),
        );
      } else {
        const failMsg = lm("taskLog.gate.cmdFailed", { cmd, exitCode: gateRes.exitCode, outputTail: gateRes.outputTail || gateRes.errorMessage || lmText("taskLog.gate.execFailed") });
        executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, failMsg);
        failureOutputs.push(renderLogMsg(failMsg));
        return {
          passed: false,
          evidence: failureOutputs.join("\n---\n"),
          results,
          aborted: false,
        };
      }
    } catch (cmdErr) {
      const errDetail = lm("taskLog.gate.cmdThrew", { cmd, cmdErr });
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, errDetail);
      failureOutputs.push(renderLogMsg(errDetail));
      return {
        passed: false,
        evidence: failureOutputs.join("\n---\n"),
        results,
        aborted: false,
      };
    }
  }

  return { passed: true, evidence: "", results, aborted: false };
}

/**
 * F10.c / S1: after terminal takeover, re-run gates from BLOCKED.
 * Pass → DONE + downstream; fail → BLOCKED.
 */
export async function completeTakeoverAndRerunGates(
  sessionId: string,
  taskId: string,
): Promise<boolean> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline) return false;
  const task = pipeline.tasks.find((t) => t.id === taskId);
  if (!task) return false;
  if (pipeline.taskStates[taskId] !== "BLOCKED") return false;

  const workdir =
    pipeline.taskCwds?.[taskId] || pipeline.baseCwd || pipeline.cwd || ".";

  executionStore.updateTaskState(
    sessionId,
    taskId,
    "VERIFYING",
    lm("taskLog.takeover.rerunGate"),
  );

  const gate = await runGatesForTask({ sessionId, taskId, task, workdir });
  if (gate.aborted) return false;

  if (gate.passed) {
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "DONE",
      lm("taskLog.takeover.done"),
    );
    void recordOutcomeForTask(sessionId, task, "USER_TAKEOVER", pipeline.taskTurnIds?.[taskId] ?? taskId, null);
    await progressDownstreamTasks(sessionId);
    return true;
  }

  executionStore.updateTaskState(
    sessionId,
    taskId,
    "BLOCKED",
    lm("taskLog.blocked.takeoverGateFailed", { p0: gate.evidence.slice(0, 300) }),
  );
  return false;
}

/** Resolve pasteable resume command for a blocked task (F10.c). */
export async function getTaskTerminalResumeCommand(
  sessionId: string,
  taskId: string,
): Promise<{ command: string | null; hint?: string }> {
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (!pipeline) return { command: null, hint: lm("taskLog.resumeHint.noPipeline") };

  const providerRaw =
    pipeline.taskProviders?.[taskId] ||
    pipeline.assignments?.find((a) => a.taskId === taskId)?.provider ||
    null;
  const threadId = pipeline.threadIds?.[taskId] ?? null;
  const cwd =
    pipeline.taskCwds?.[taskId] || pipeline.baseCwd || pipeline.cwd || ".";

  if (!providerRaw) {
    return { command: null, hint: lm("taskLog.resumeHint.unknownProvider") };
  }
  if (providerRaw === "AIDER") {
    return {
      command: null,
      hint: lm("taskLog.resumeHint.aider", { cwd }),
    };
  }
  if (!threadId) {
    return { command: null, hint: lm("taskLog.resumeHint.noThreadId") };
  }

  try {
    const command = await terminalResumeCommand(
      providerRaw as ProviderId,
      threadId,
      cwd,
    );
    if (!command) {
      return { command: null, hint: lm("taskLog.resumeHint.none") };
    }
    return { command };
  } catch (err) {
    return { command: null, hint: lm("taskLog.resumeHint.failed", { err }) };
  }
}

/**
 * Handles sub-agent CLI completion: executes acceptance gates, generates report,
 * and advances the DAG downstream (m4b & m4c).
 */
export async function processSubAgentCompletion(
  ctx: RunningSubAgentContext,
  doneEvent?: { costUsd?: number | null },
): Promise<void> {
  const executionStore = useExecution.getState();

  if (pipelineTaskState(ctx.sessionId, ctx.taskId) !== "RUNNING") {
    executionStore.appendTaskLog(
      ctx.sessionId,
      ctx.taskId,
      lm("taskLog.ignore.completionAborted"),
    );
    activeSubAgents.delete(ctx.turnId);
    return;
  }

  // 1. Transition to VERIFYING (m4b)
  executionStore.updateTaskState(
    ctx.sessionId,
    ctx.taskId,
    "VERIFYING",
    lm("taskLog.gate.start"),
  );

  // 1b. R6 write-scope check before any gate command (F5.i)
  executionStore.recordTaskFailureEvidence(ctx.sessionId, ctx.taskId, null);
  try {
    const outOfScopeFiles = await findOutOfScopeFiles(ctx);
    if (pipelineTaskState(ctx.sessionId, ctx.taskId) !== "VERIFYING") {
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.ignore.scopeResultDropped"),
      );
      activeSubAgents.delete(ctx.turnId);
      return;
    }
    if (outOfScopeFiles.length > 0) {
      const listing = outOfScopeFiles.join(", ");
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.scope.violation", { p0: ctx.task.write_scope.join(", ") || lmRef("taskLog.none"), p1: outOfScopeFiles.join("\n") }),
      );
      // F13.c / R6: same repair ladder as gate failure
      await applyRepairLadder(ctx, {
        failedCommand: null,
        outputTail: lmText("taskLog.scope.outputTail", { listing }),
        outOfScopeFiles,
        previousChangedFiles: [...ctx.changedFiles],
      });
      return;
    }
  } catch (scopeErr) {
    console.warn("[TaskRunner] Write-scope check failed; continuing with the verification gate:", scopeErr);
    executionStore.appendTaskLog(
      ctx.sessionId,
      ctx.taskId,
      lm("taskLog.scope.checkError", { scopeErr }),
    );
  }

  // 2. Execute acceptance gate commands (m4b / F10.d shared)
  const commands = ctx.task.acceptance?.commands ?? [];
  const gate = await runGatesForTask({
    sessionId: ctx.sessionId,
    taskId: ctx.taskId,
    task: ctx.task,
    workdir: ctx.worktreeCwd,
    attemptId: ctx.turnId,
  });
  if (gate.aborted) {
    activeSubAgents.delete(ctx.turnId);
    return;
  }

  if (pipelineTaskState(ctx.sessionId, ctx.taskId) !== "VERIFYING") {
    executionStore.appendTaskLog(
      ctx.sessionId,
      ctx.taskId,
      lm("taskLog.ignore.finalizeAborted"),
    );
    activeSubAgents.delete(ctx.turnId);
    return;
  }

  const allPassed = gate.passed;
  const failureOutputs = gate.evidence ? [gate.evidence] : [];

  // 3. Gate results evaluation, TaskReport parsing, and DAG progression (m4c)
  if (allPassed) {
    const gateResultsSummary =
      commands.length > 0
        ? `All ${commands.length} gate command(s) passed`
        : "No extra gate commands";

    const changedFilesStat =
      ctx.changedFiles.length > 0
        ? `${ctx.changedFiles.length} file(s) modified/added`
        : "No file changes captured";

    let report: TaskReport;
    try {
      report = await parseOrGenerateTaskReport(
        ctx.task,
        ctx.rawOutput,
        ctx.task.assigned_model || "unknown",
        ctx.turnId,
        true,
        changedFilesStat,
        ctx.changedFiles,
        gateResultsSummary,
        doneEvent?.costUsd ?? null,
      );
    } catch (repErr) {
      console.warn("[TaskRunner] Failed to parse the task report; falling back to the local default report:", repErr);
      report = {
        taskId: ctx.taskId,
        title: ctx.task.title,
        model: ctx.task.assigned_model || "unknown",
        attemptId: ctx.turnId,
        firstPass: true,
        isLocallyGenerated: true,
        completedWork: ctx.rawOutput || "Sub-agent task executed",
        tipsForDownstream: REPORT_NONE,
        selfCheckCriteria: [],
        changedFilesStat,
        changedFilesList: ctx.changedFiles,
        gateResultsSummary,
        costUsd: doneEvent?.costUsd ?? null,
        defects: null,
      };
    }

    // F13.e: tester defects → implementation fix tasks (before DONE so deps see test DONE)
    if (roleFromDomain(ctx.task.domain) === "TESTER" && report.defects) {
      await injectDefectFixTasks(
        ctx.sessionId,
        ctx.task,
        report.defects,
        report.gateResultsSummary || gateResultsSummary,
      );
    }

    // Record upstream artifact for downstream tasks
    const artifact: UpstreamArtifact = {
      taskId: ctx.taskId,
      title: ctx.task.title,
      report: report.completedWork || ctx.task.title,
      changedFiles: report.changedFilesList || ctx.changedFiles,
    };
    executionStore.recordTaskArtifact(ctx.sessionId, ctx.taskId, artifact);

    // Record TaskReport to SQLite
    try {
      await recordTaskReport({
        taskId: ctx.taskId,
        modelId: ctx.task.assigned_model || "unknown",
        attemptId: ctx.turnId,
        firstPass: true,
        isLocallyGenerated: report.isLocallyGenerated,
        summaryMd: report.completedWork,
        fullMd: JSON.stringify(report),
        contractDeviation: report.contractDeviations ?? null,
        changedFilesStat: report.changedFilesStat,
        changedFilesList: report.changedFilesList,
        gateResults: report.gateResultsSummary,
        costUsd: report.costUsd ?? null,
      });
    } catch {
      // non-fatal in mock environment
    }

    // F11.d/e: worktree mode — commit + merge into integration (or spawn conflict task)
    if (useSettings.getState().isolation === "worktree") {
      const wtResult = await finishWorktreeTask(ctx.sessionId, ctx.task);
      if (!wtResult.ok) {
        executionStore.updateTaskState(
          ctx.sessionId,
          ctx.taskId,
          "BLOCKED",
          lm("taskLog.blocked.worktreeCommitFailed", { p0: wtResult.error.slice(0, 300) }),
        );
        activeSubAgents.delete(ctx.turnId);
        void recordAttemptEnd(ctx.turnId, "FAILED");
        syncMainSessionState(ctx.sessionId);
        return;
      }
    }

    // Mark task DONE
    const snippet = (report.completedWork || "").slice(0, 100);
    const repairStep = executionStore.pipelines[ctx.sessionId]?.repairSteps?.[ctx.taskId];
    executionStore.updateTaskState(
      ctx.sessionId,
      ctx.taskId,
      "DONE",
      lm("taskLog.done.delivered", { p0: snippet ? `: ${snippet}` : "" }),
    );
    void recordAttemptEnd(ctx.turnId, "DONE");
    void recordOutcomeForTask(
      ctx.sessionId,
      ctx.task,
      outcomeResultForCompletion(repairStep),
      ctx.turnId,
      report.costUsd ?? doneEvent?.costUsd ?? null,
    );

    // F9.e: non-empty contractDeviations (not `none`) pause transitive downstream
    const deviation = (report.contractDeviations ?? "").trim();
    if (deviation && !isNoneToken(deviation)) {
      executionStore.addDeviationSource(ctx.sessionId, ctx.taskId);
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        lm("taskLog.deviation.paused", { p0: deviation.slice(0, 200) }),
      );
    }

    activeSubAgents.delete(ctx.turnId);
    syncMainSessionState(ctx.sessionId);
    await maybeCheckPlanBudget(ctx.sessionId);

    // Progress DAG to advance downstream tasks
    await progressDownstreamTasks(ctx.sessionId);
  } else {
    const failureEvidence = failureOutputs.join("\n---\n") || "An acceptance command exited non-zero";
    const failedCmd =
      gate.results.find((r) => !r.passed)?.command ??
      commands[commands.length - 1] ??
      null;
    await applyRepairLadder(ctx, {
      failedCommand: failedCmd,
      outputTail: failureEvidence,
      outOfScopeFiles: [],
      previousChangedFiles: [...ctx.changedFiles],
    });
  }
}

/**
 * Advances the DAG after a task completes (F9: promotion happens inside `dispatchReadyTasks`).
 */
export async function progressDownstreamTasks(sessionId: string): Promise<string[]> {
  const launched = await dispatchReadyTasks(sessionId);
  void maybeStartPlanDelivery(sessionId);
  return launched;
}

/** Collect uniqued acceptance commands across all tasks (F11.f / 03 §9 until F16). */
function collectFullGateCommands(tasks: TaskItem[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const t of tasks) {
    for (const c of t.acceptance?.commands ?? []) {
      const cmd = c.trim();
      if (!cmd || seen.has(cmd)) continue;
      seen.add(cmd);
      out.push(cmd);
    }
  }
  return out;
}

/**
 * F11.f: when the pipeline is DONE, run full gates + delivery summary (worktree → three-way UI).
 */
export async function maybeStartPlanDelivery(sessionId: string): Promise<void> {
  const store = useExecution.getState();
  const pipeline = store.pipelines[sessionId];
  if (!pipeline || pipeline.status !== "DONE") return;
  if (pipeline.delivery && pipeline.delivery.phase !== "idle" && pipeline.delivery.phase !== "failed") {
    return;
  }
  if (deliveryInFlight.has(sessionId)) return;
  deliveryInFlight.add(sessionId);

  const logTaskId = pipeline.tasks[0]?.id ?? "_";
  const append = (msg: string) => store.appendTaskLog(sessionId, logTaskId, msg);

  try {
    store.setDelivery(sessionId, { phase: "running_gates" });
    append(lm("taskLog.delivery.fullGateStart"));

    const isolation = useSettings.getState().isolation;
    const workdir =
      isolation === "worktree" && pipeline.integration?.root
        ? pipeline.integration.root
        : pipeline.baseCwd || pipeline.cwd || ".";

    const commands = collectFullGateCommands(pipeline.tasks);
    let fullGatePassed = true;
    let fullGateResult: GateCommandResult | null = null;
    const envAllow = useSettings.getState().gateEnvAllow ?? [];

    for (const cmd of commands) {
      append(lm("taskLog.delivery.gateRunning", { cmd }));
      try {
        const gateRes = await executeGateCommand(
          cmd,
          workdir,
          600,
          true,
          envAllow,
          `gate:delivery:${sessionId}`,
        );
        fullGateResult = gateRes;
        store.recordGateResult(sessionId, {
          taskId: "_delivery",
          command: cmd,
          passed: gateRes.passed,
          exitCode: gateRes.exitCode,
          durationMs: gateRes.durationMs,
          outputTail: gateRes.outputTail || "",
        });
        if (!gateRes.passed) {
          fullGatePassed = false;
          append(
            lm("taskLog.delivery.gateFailed", { cmd, exitCode: gateRes.exitCode, outputTail: (gateRes.outputTail || "").slice(0, 200) }),
          );
          break;
        }
        append(lm("taskLog.delivery.gatePassed", { cmd }));
      } catch (err) {
        fullGatePassed = false;
        append(lm("taskLog.delivery.gateError", { cmd, err }));
        break;
      }
    }

    if (commands.length === 0) {
      append(lm("taskLog.delivery.noCommands"));
    }

    // Build TaskReport list from DB rows (best-effort)
    const reports: TaskReport[] = [];
    for (const t of pipeline.tasks) {
      try {
        const rows = await getTaskReports(t.id);
        const latest = rows[rows.length - 1];
        if (latest) {
          reports.push({
            taskId: latest.taskId,
            title: t.title,
            model: latest.modelId,
            attemptId: latest.attemptId,
            firstPass: latest.firstPass,
            isLocallyGenerated: latest.isLocallyGenerated,
            completedWork: latest.summaryMd,
            contractDeviations: latest.contractDeviation,
            tipsForDownstream: "",
            selfCheckCriteria: [],
            changedFilesStat: latest.changedFilesStat,
            changedFilesList: latest.changedFilesList,
            gateResultsSummary: latest.gateResults,
            costUsd: latest.costUsd,
          });
          continue;
        }
      } catch {
        // fall through to artifact stub
      }
      const art = pipeline.upstreamArtifacts?.[t.id];
      reports.push({
        taskId: t.id,
        title: t.title,
        model: t.assigned_model || "unknown",
        attemptId: pipeline.taskTurnIds?.[t.id] || t.id,
        firstPass: true,
        isLocallyGenerated: true,
        completedWork: art?.report || t.title,
        tipsForDownstream: "",
        selfCheckCriteria: [],
        changedFilesStat: "",
        changedFilesList: art?.changedFiles ?? [],
        gateResultsSummary: "",
        costUsd: pipeline.taskCostUsd?.[t.id] ?? null,
      });
    }

    const cwd = pipeline.baseCwd || pipeline.cwd || ".";
    let plan: Awaited<ReturnType<typeof loadPlan>>;
    try {
      plan = await loadPlan(cwd, sessionId);
      if (!plan.tasks?.length) {
        plan = { ...plan, tasks: pipeline.tasks, contracts: pipeline.contracts ?? plan.contracts };
      }
    } catch {
      plan = {
        version: 1,
        spec: {
          raw: "",
          goal: pipeline.tasks[0]?.goal || lmText("taskLog.delivery.defaultGoal"),
          inScope: [],
          outOfScope: [],
          decisions: [],
        },
        tasks: pipeline.tasks,
        contracts: pipeline.contracts ?? [],
        rawMarkdown: "",
      };
    }

    const summary = await generateDeliverySummary(
      plan,
      reports,
      fullGateResult,
      0,
      false,
      pipeline.planDbId != null ? String(pipeline.planDbId) : null,
    );

    store.setDelivery(sessionId, {
      phase: "ready",
      summary,
      fullGatePassed,
      error: fullGatePassed ? null : lm("taskLog.delivery.gateNotPassed"),
    });
    append(
      fullGatePassed
        ? lm("taskLog.delivery.ready", { length: summary.touchedFiles.length, p1: summary.actualCostUsd.toFixed(2) })
        : lm("taskLog.delivery.readyGateFailed"),
    );

    // F14.g: settle assignment savings when plan reaches terminal delivery
    if (pipeline.planDbId != null) {
      try {
        const refModel = useSettings.getState().referenceModel;
        const override =
          refModel && refModel !== "auto" ? refModel : undefined;
        const eventId = await settlePlanSavings(pipeline.planDbId, override);
        if (eventId != null) {
          append(lm("taskLog.delivery.savingsRecorded", { eventId }));
        }
      } catch (err) {
        console.warn("[TaskRunner] settle_plan_savings failed:", err);
      }
    }
  } catch (err) {
    store.setDelivery(sessionId, {
      phase: "failed",
      error: String(err),
    });
    append(lm("taskLog.delivery.failed", { err }));
  } finally {
    deliveryInFlight.delete(sessionId);
  }
}

/** F11.f: merge integration into main checkout (requires clean tree + confirm). */
export async function deliverMergeToCheckout(sessionId: string): Promise<boolean> {
  const store = useExecution.getState();
  const pipeline = store.pipelines[sessionId];
  if (!pipeline?.integration) return false;
  const projectPath = pipeline.baseCwd || pipeline.cwd;
  if (!projectPath) return false;
  const logTaskId = pipeline.tasks[0]?.id ?? "_";

  try {
    await mergeIntegrationToCheckout(projectPath, pipeline.integration.branch);
    // Clean integration worktree; delete task/integration branches after merge.
    try {
      await removeTaskWorktree(
        pipeline.integration.root,
        pipeline.integration.branch,
        false,
        true,
      );
    } catch (err) {
      store.appendTaskLog(sessionId, logTaskId, lm("taskLog.delivery.cleanupIntegrationFailed", { err }));
    }
    try {
      await pruneProjectWorktrees(projectPath);
    } catch {
      /* best-effort */
    }
    store.setIntegration(sessionId, null);
    store.setDelivery(sessionId, {
      ...(pipeline.delivery ?? { phase: "ready" }),
      phase: "merged",
      error: null,
    });
    store.appendTaskLog(
      sessionId,
      logTaskId,
      lm("taskLog.delivery.merged", { branch: pipeline.integration.branch }),
    );
    return true;
  } catch (err) {
    const code = String(err);
    let msg = code;
    if (code.includes("DIRTY")) {
      msg = lm("taskLog.delivery.mergeDirty");
    } else if (code.includes("CONFLICT")) {
      msg = lm("taskLog.delivery.mergeConflict");
    }
    store.setDelivery(sessionId, {
      ...(pipeline.delivery ?? { phase: "ready" }),
      phase: "ready",
      error: msg,
    });
    store.appendTaskLog(sessionId, logTaskId, lm("taskLog.delivery.mergeFailed", { msg }));
    return false;
  }
}

/** F11.f: write integration diff to a patch file under the project. */
export async function deliverExportPatch(sessionId: string): Promise<string | null> {
  const store = useExecution.getState();
  const pipeline = store.pipelines[sessionId];
  if (!pipeline?.integration) return null;
  const projectPath = pipeline.baseCwd || pipeline.cwd;
  if (!projectPath) return null;
  const logTaskId = pipeline.tasks[0]?.id ?? "_";

  const fromRef =
    pipeline.integration.baseCommit ||
    (await gitRevParse(projectPath, "HEAD").catch(() => "HEAD"));
  const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  const outPath = `${projectPath}/.tokage/delivery-${sid8(sessionId)}-${stamp}.patch`;

  try {
    await exportIntegrationPatch(
      projectPath,
      fromRef,
      pipeline.integration.branch,
      outPath,
    );
    store.setDelivery(sessionId, {
      ...(pipeline.delivery ?? { phase: "ready" }),
      phase: "exported",
      patchPath: outPath,
      error: null,
    });
    store.appendTaskLog(sessionId, logTaskId, lm("taskLog.delivery.patchExported", { outPath }));
    return outPath;
  } catch (err) {
    store.setDelivery(sessionId, {
      ...(pipeline.delivery ?? { phase: "ready" }),
      phase: "ready",
      error: lm("taskLog.delivery.exportFailed", { err }),
    });
    store.appendTaskLog(sessionId, logTaskId, lm("taskLog.delivery.exportFailedLog", { err }));
    return null;
  }
}

/** F11.f: keep integration branch; remove worktree directory only. */
export async function deliverKeepBranch(sessionId: string): Promise<void> {
  const store = useExecution.getState();
  const pipeline = store.pipelines[sessionId];
  if (!pipeline?.integration) return;
  const projectPath = pipeline.baseCwd || pipeline.cwd || ".";
  const logTaskId = pipeline.tasks[0]?.id ?? "_";
  const { branch, root } = pipeline.integration;

  try {
    await removeTaskWorktree(root, branch, true, true);
  } catch (err) {
    store.appendTaskLog(sessionId, logTaskId, lm("taskLog.delivery.cleanupWorktreeFailed", { err }));
  }
  try {
    await pruneProjectWorktrees(projectPath);
  } catch {
    /* best-effort */
  }
  store.setIntegration(sessionId, null);
  store.setDelivery(sessionId, {
    ...(pipeline.delivery ?? { phase: "ready" }),
    phase: "kept",
    error: null,
  });
  store.appendTaskLog(
    sessionId,
    logTaskId,
    lm("taskLog.delivery.branchKept", { branch }),
  );
}

/** Human-readable wait reason for READY tasks blocked by the scheduler (F9.d). */
export function formatTaskWaitReason(reason: DispatchIneligibleReason): string {
  if ("globalSlotsFull" in reason) {
    const { current, max } = reason.globalSlotsFull;
    return lm("taskLog.wait.globalFull", { current, max });
  }
  if ("providerSlotsFull" in reason) {
    const { provider, current, max } = reason.providerSlotsFull;
    return lm("taskLog.wait.providerFull", { provider, current, max });
  }
  if ("writeScopeCollision" in reason) {
    const { conflictingTaskId } = reason.writeScopeCollision;
    return lm("taskLog.wait.scopeConflict", { conflictingTaskId });
  }
  if ("contractDeviationPaused" in reason) {
    const { sourceTaskId } = reason.contractDeviationPaused;
    return lm("taskLog.wait.upstreamDeviation", { sourceTaskId });
  }
  return lm("taskLog.wait.constraint");
}

function buildPickAssignments(
  pipeline: {
    tasks: TaskItem[];
    assignments?: { taskId: string; provider: string; modelId: string }[];
  },
): Record<string, [string, string]> {
  const out: Record<string, [string, string]> = {};
  if (pipeline.assignments) {
    for (const a of pipeline.assignments) {
      out[a.taskId] = [a.provider, a.modelId];
    }
  }
  for (const t of pipeline.tasks) {
    if (out[t.id]) continue;
    const spec = t.assigned_model || "";
    const colon = spec.indexOf(":");
    if (colon > 0) {
      out[t.id] = [spec.slice(0, colon), spec.slice(colon + 1)];
    } else {
      out[t.id] = ["mock", spec || "default"];
    }
  }
  return out;
}

/**
 * Single dispatch entry (F1.h / F9): pause check, Rust pick, claim/failure handling.
 * `opts.only` limits launch to the intersection with `dispatchable`.
 */
export async function dispatchReadyTasks(
  sessionId: string,
  opts?: { only?: string[] },
): Promise<string[]> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline || pipeline.status === "PAUSED" || pipeline.status === "DONE") {
    return [];
  }
  // After an interrupt (or restart) only an explicit continue/retry of named tasks may launch.
  if (pipeline.status === "INTERRUPTED" && !opts?.only) return [];

  const { maxParallel, isolation } = useSettings.getState();
  const maxConcurrency = Math.max(1, maxParallel || 3);
  const useWorktree = isolation === "worktree";

  let pick;
  try {
    pick = await pickDispatchableTasks({
      tasks: pipeline.tasks,
      taskStates: pipeline.taskStates,
      assignments: buildPickAssignments(pipeline),
      maxGlobalConcurrent: maxConcurrency,
      deviationSources: pipeline.deviationSources ?? [],
    });
  } catch (err) {
    console.error("[TaskRunner] pickDispatchableTasks failed:", err);
    return [];
  }

  // Apply promotions first (PENDING → READY)
  for (const id of pick.promoted) {
    if (opts?.only && !opts.only.includes(id)) continue;
    const state = useExecution.getState().pipelines[sessionId]?.taskStates[id];
    if (state === "PENDING") {
      const task = pipeline.tasks.find((t) => t.id === id);
      const deps = task?.depends_on?.join(", ") ?? "";
      executionStore.updateTaskState(
        sessionId,
        id,
        "READY",
        lm("taskLog.ready.depsDone", { p0: deps ? ` (${deps})` : "" }),
      );
    }
  }

  executionStore.setTaskWaitReasons(sessionId, pick.ineligible);

  let toLaunch = pick.dispatchable;
  if (opts?.only?.length) {
    const only = new Set(opts.only);
    toLaunch = toLaunch.filter((id) => only.has(id));
  }

  const fresh = useExecution.getState().pipelines[sessionId];
  if (!fresh) return [];

  const launchedTurnIds: string[] = [];
  const baseCwd = fresh.baseCwd || fresh.cwd || ".";
  const contracts = fresh.contracts || [];
  const upstreamArtifactsList: UpstreamArtifact[] = Object.values(fresh.upstreamArtifacts || {});

  for (const taskId of toLaunch) {
    const task = fresh.tasks.find((t) => t.id === taskId);
    if (!task) continue;
    if (fresh.taskStates[taskId] !== "READY") continue;
    try {
      const taskCwd = fresh.taskCwds?.[task.id] || baseCwd;
      const res = await dispatchSubAgentTask({
        sessionId,
        task,
        contracts,
        upstreamArtifacts: upstreamArtifactsList,
        baseCwd: taskCwd,
        useWorktree: useWorktree && !fresh.taskCwds?.[task.id],
      });
      launchedTurnIds.push(res.turnId);
    } catch (err) {
      console.error(`[TaskRunner] Failed to start task ${task.id}:`, err);
      const state = useExecution.getState().pipelines[sessionId]?.taskStates[task.id];
      if (state !== "BLOCKED") {
        executionStore.updateTaskState(
          sessionId,
          task.id,
          "BLOCKED",
          lm("taskLog.error.spawnFailed", { err }),
        );
      }
    }
  }

  return launchedTurnIds;
}

/** Confirm a contract deviation and re-dispatch downstream (F9.e). */
export async function confirmContractDeviation(
  sessionId: string,
  sourceTaskId: string,
): Promise<string[]> {
  useExecution.getState().confirmDeviationSource(sessionId, sourceTaskId);
  useExecution.getState().appendTaskLog(
    sessionId,
    sourceTaskId,
    lm("taskLog.deviation.confirmed"),
  );
  return dispatchReadyTasks(sessionId);
}
