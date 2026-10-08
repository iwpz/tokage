import type {
  ChatEvent,
  DispatchIneligibleReason,
  FailureEvidence,
  PipelineGateResult,
  Plan,
  PlanContract,
  ProviderId,
  ProviderInfo,
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
import { runnable, selectableGroups } from "./models";
import { useChat } from "../stores/chat";
import { useExecution, type TaskApproval } from "../stores/execution";
import { useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";
import { pushUiEvent } from "./uiEvents";
import { findReusableWindow } from "./windowReuse";

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

const REPAIR_STEP_LABEL: Record<RepairStep, string> = {
  INITIAL: "初始",
  IN_WINDOW_REPAIR: "同窗口修复",
  REASSIGNED: "改派",
  BLOCKED: "阻塞",
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
    if (!trimmed || trimmed === "无") continue;
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
        `[缺陷回路] 已生成修复任务 ${fix.id}（来自 ${entry.implTaskId}: ${entry.description.slice(0, 80)}）`,
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
        `[缺陷回路] 等待上游测试任务完成后调度`,
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
        `[预算红线] ${check.message}`,
      );
    } else if (
      (check.is50PctOverrun || check.isRepairOverrun || check.isRedlineExceeded) &&
      !check.shouldPauseQueue
    ) {
      const logTaskId = pipeline.tasks[0]?.id ?? "_";
      useExecution.getState().appendTaskLog(
        sessionId,
        logTaskId,
        `[预算提示] ${check.message}`,
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
          `[额度暂停] 单模型模式锁定 ${locked.provider}:${locked.model}；Provider ${e.provider} 熔断（${e.reason}），无法改派`,
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
            `[改派] Provider ${e.provider} 熔断（${e.reason}），未开始任务改派至 ${modelSpec}`,
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
        `[恢复] Provider ${e.provider} 已恢复，任务回到 READY`,
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
      `[任务阻塞] 修复决策失败: ${err}`,
    );
    syncMainSessionState(sessionId);
    return "blocked";
  }

  executionStore.setRepairStep(sessionId, taskId, nextStep);
  const n = repairStepOrdinal(nextStep);
  executionStore.appendTaskLog(
    sessionId,
    taskId,
    `[修复梯级] 第 ${n} 步：${REPAIR_STEP_LABEL[nextStep]}（由 ${REPAIR_STEP_LABEL[currentStep]}）`,
  );

  // Degrade in-window repair without thread id → reassign
  let effective = decision;
  if ("tryInWindowRepair" in decision) {
    const threadId = pipeline.threadIds?.[taskId];
    if (!threadId) {
      executionStore.appendTaskLog(
        sessionId,
        taskId,
        "[修复梯级] 无 thread id，同窗口修复降级为改派",
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
      `[同窗口修复] 准备在同一 CLI 会话续接修复…`,
    );

    const turnId = `subagent-${sessionId}-${taskId}-${Date.now().toString(36)}`;
    const claimed = executionStore.claimTaskRunning(
      sessionId,
      taskId,
      turnId,
      `[同窗口修复] 启动子代理 (Turn: ${turnId})`,
    );
    if (!claimed) {
      executionStore.updateTaskState(
        sessionId,
        taskId,
        "BLOCKED",
        "[任务阻塞] 同窗口修复认领失败",
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
    const { providerEnabled, providerTrust, blockedModels } = useSettings.getState();
    const resolved = resolveTaskProviderAndModel(
      task.assigned_model,
      providers,
      providerEnabled,
      providerTrust,
      blockedModels,
    );
    if (!resolved) {
      failClaimedSubAgent(sessionId, taskId, turnId, "[系统错误] 同窗口修复无法解析模型");
      syncMainSessionState(sessionId);
      return "blocked";
    }

    const trustedList: ProviderId[] =
      project?.trusted === true
        ? (Object.entries(useSettings.getState().providerTrust)
            .filter(([, v]) => v)
            .map(([k]) => k as ProviderId))
        : [resolved.provider];

    const prompt = `## 修复指示（同窗口）\n请根据下列失败证据修复问题，完成后重新输出任务报告。\n\n${evidenceMd}`;
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
      failClaimedSubAgent(sessionId, taskId, turnId, `[系统错误] 同窗口修复启动失败: ${err}`);
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
        `[任务阻塞] 单模型模式锁定 ${locked.provider}:${locked.model}，跳过改派。失败证据：\n${evidenceMd}`,
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
      `[改派] 排除 ${excludeModels.join(", ") || "无"}，重新分配模型…`,
    );

    const plan = planFromPipeline(sessionId);
    if (!plan) {
      executionStore.updateTaskState(sessionId, taskId, "BLOCKED", "[任务阻塞] 改派失败：无计划");
      syncMainSessionState(sessionId);
      return "blocked";
    }

    let assignment;
    try {
      assignment = await reassignTask(plan, taskId, excludeModels);
    } catch (err) {
      // F15.f: no alternate provider → PAUSED_QUOTA (await recover) instead of BLOCKED.
      if (meltInfo.melted) {
        const until = meltInfo.until ? `，恢复时间 ${meltInfo.until}` : "";
        executionStore.updateTaskState(
          sessionId,
          taskId,
          "PAUSED_QUOTA",
          `[额度暂停] Provider ${provider} 熔断（${meltInfo.reason ?? "unknown"}${until}），无可用改派目标: ${err}`,
        );
        syncMainSessionState(sessionId);
        return "paused_quota";
      }
      executionStore.updateTaskState(
        sessionId,
        taskId,
        "BLOCKED",
        `[任务阻塞] 改派失败（候选耗尽）: ${err}`,
      );
      syncMainSessionState(sessionId);
      return "blocked";
    }

    const modelSpec = `${assignment.provider}:${assignment.cliModelArg}`;
    executionStore.updateTaskAssignedModel(sessionId, taskId, modelSpec);
    // New window: startChatTurn uses resume: null (dispatchSubAgentTask); stale threadId is ok.

    const pipe2 = useExecution.getState().pipelines[sessionId];
    const freshTask = {
      ...task,
      assigned_model: modelSpec,
    };
    const contracts = pipe2?.contracts ?? [];
    const upstream = Object.values(pipe2?.upstreamArtifacts ?? {});

    executionStore.appendTaskLog(
      sessionId,
      taskId,
      `[改派] 新模型 ${modelSpec}（p=${assignment.pSuccess.toFixed(2)}）`,
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
          `[任务阻塞] 改派后启动失败: ${err}`,
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
      `[任务阻塞] ${reason}\n接管命令: ${cmd}`,
    );
    syncMainSessionState(sessionId);
    return "blocked";
  }

  activeSubAgents.delete(ctx.turnId);
  executionStore.updateTaskState(sessionId, taskId, "BLOCKED", "[任务阻塞] 未知修复决策");
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
  const kind = idle ? "空闲超时" : "硬超时";
  return `[任务阻塞] ${kind}（${minutes} 分钟）`;
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
  // Soft existence check: in browser/tests we keep the path; if clearly empty, fall back.
  if (!taskCwd.trim()) {
    taskCwd = baseCwd;
    executionStore.appendTaskLog(
      sessionId,
      taskId,
      `[继续] 任务工作目录缺失，回退到会话目录 ${baseCwd}`,
    );
  } else if (taskCwd !== baseCwd && !pipeline.taskCwds?.[taskId]) {
    executionStore.appendTaskLog(
      sessionId,
      taskId,
      `[继续] 使用会话目录 ${taskCwd}`,
    );
  }

  // Mark pipeline running again when user continues.
  if (pipeline.status === "INTERRUPTED") {
    executionStore.resumeExecution(sessionId);
  }

  const threadId = pipeline.threadIds?.[taskId];
  if (!threadId) {
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "READY",
      "[继续] 无 CLI 会话 ID，重新派发新窗口",
    );
    await dispatchReadyTasks(sessionId, { only: [taskId] });
    return;
  }

  // F16.e: prefer queued main-session follow-up for this task when present.
  const { takePendingTaskFollowUp } = await import("./appendTriage");
  const followUp = takePendingTaskFollowUp(sessionId, taskId);
  const prompt =
    followUp?.trim() ||
    "继续未完成的任务；若上一轮已完成，请直接输出任务报告";

  executionStore.updateTaskState(
    sessionId,
    taskId,
    "READY",
    `[继续] 准备以 resume=${threadId} 续接`,
  );

  const { providers } = useProviders.getState();
  const { providerEnabled, providerTrust, blockedModels } = useSettings.getState();
  const resolved = resolveTaskProviderAndModel(
    task.assigned_model,
    providers,
    providerEnabled,
    providerTrust,
    blockedModels,
  );
  if (!resolved) {
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "BLOCKED",
      `[系统错误] 继续失败：无法解析模型（assigned_model=${task.assigned_model ?? "null"}）`,
    );
    return;
  }

  const turnId = `subagent-${sessionId}-${taskId}-${Date.now().toString(36)}`;
  const claimed = executionStore.claimTaskRunning(
    sessionId,
    taskId,
    turnId,
    `[继续] 恢复子代理 (Turn: ${turnId})`,
  );
  if (!claimed) {
    executionStore.appendTaskLog(sessionId, taskId, "[继续] 认领失败，任务状态已变");
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
    failClaimedSubAgent(sessionId, taskId, turnId, `[系统错误] 继续任务异常: ${err}`);
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
    `[用户取消] 任务 ${taskId} 已取消`,
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
        `[跳过] 上游任务 ${taskId} 已取消`,
      );
    }
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
      `[等待审批] ${cmds}`,
    );
    activeSubAgents.delete(e.turnId);
    return;
  }

  executionStore.updateTaskState(
    ctx.sessionId,
    ctx.taskId,
    "BLOCKED",
    "[执行异常] CLI 异常退出（无终止事件）",
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
      `[审批] 已记录 ${decision}，等待本轮 CLI 结束后续接`,
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
      "[审批] 无 CLI 会话 ID，无法续接；请重新执行该任务",
    );
    return;
  }

  const prompt =
    decision === "deny"
      ? `User denied command: ${command}. Please continue without running this command or find an alternative approach.`
      : `User approved command: ${command}. Please execute it now.`;

  executionStore.updateTaskState(sessionId, taskId, "READY", `[审批续接] 准备恢复执行 (${decision})`);

  const sessionAllowed = pipeline.taskAllowed?.[taskId] ?? [];
  const worktreeCwd =
    pipeline.taskCwds?.[taskId] || pipeline.baseCwd || pipeline.cwd || ".";

  // Re-dispatch through TaskRunner path so AGENT timeouts / write_scope apply.
  // Mark READY first so claimTaskRunning succeeds; then start with resume.
  const { providers } = useProviders.getState();
  const { providerEnabled, providerTrust, blockedModels } = useSettings.getState();
  const resolved = resolveTaskProviderAndModel(
    task.assigned_model,
    providers,
    providerEnabled,
    providerTrust,
    blockedModels,
  );
  if (!resolved) {
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "BLOCKED",
      `[系统错误] 审批续接失败：无法解析模型`,
    );
    return;
  }

  const turnId = `subagent-${sessionId}-${taskId}-${Date.now().toString(36)}`;
  const claimed = executionStore.claimTaskRunning(
    sessionId,
    taskId,
    turnId,
    `[审批续接] 恢复子代理 (Turn: ${turnId})`,
  );
  if (!claimed) {
    executionStore.appendTaskLog(sessionId, taskId, "[审批续接] 认领失败，任务状态已变");
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
    failClaimedSubAgent(sessionId, taskId, turnId, `[系统错误] 审批续接异常: ${err}`);
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
 */
export function resolveTaskProviderAndModel(
  assignedModelSpec: string | null | undefined,
  providers: ProviderInfo[],
  enabled: Partial<Record<ProviderId, boolean>> = {},
  trust: Partial<Record<ProviderId, boolean>> = {},
  blockedModels: string[] = [],
): ResolvedModel | null {
  const spec = (assignedModelSpec || "").trim();
  if (!spec) return null;

  // 1. Separate explicit prefix if provided (e.g. "CLAUDE_CODE:sonnet" or "anthropic:claude-3-7-sonnet")
  let targetProviderPrefix: string | null = null;
  let targetModelName = spec;

  if (spec.includes(":")) {
    const parts = spec.split(":");
    targetProviderPrefix = parts[0].trim();
    targetModelName = parts.slice(1).join(":").trim();
  }

  // Helper: map common provider aliases/prefixes to ProviderId
  const normalizeProvider = (name: string): ProviderId | null => {
    const s = name.toUpperCase().replace(/-/g, "_");
    if (s === "ANTHROPIC" || s === "CLAUDE" || s === "CLAUDE_CODE") return "CLAUDE_CODE";
    if (s === "OPENAI" || s === "CODEX" || s === "CODEX_CLI") return "CODEX_CLI";
    if (s === "CURSOR" || s === "CURSOR_CLI") return "CURSOR_CLI";
    if (s === "GITHUB" || s === "COPILOT" || s === "COPILOT_CLI") return "COPILOT_CLI";
    if (s === "GOOGLE" || s === "ANTIGRAVITY" || s === "ANTIGRAVITY_CLI") return "ANTIGRAVITY_CLI";
    if (s === "AIDER") return "AIDER";
    if (s === "OPENCODE") return "OPENCODE";
    if (s === "GROK" || s === "GROK_CLI") return "GROK_CLI";
    if (s === "KIRO" || s === "KIRO_CLI") return "KIRO_CLI";
    if (s === "QWEN" || s === "QWEN_CODE") return "QWEN_CODE";
    if (s === "KIMI" || s === "KIMI_CODE") return "KIMI_CODE";
    return null;
  };

  const parsedProviderId = targetProviderPrefix ? normalizeProvider(targetProviderPrefix) : null;
  const groups = selectableGroups(providers, enabled, trust, "direct", blockedModels);

  // Explicit provider prefix: must resolve within that provider's catalog
  if (parsedProviderId) {
    const group = groups.find((g) => g.provider.id === parsedProviderId);
    if (!group || !runnable(group.provider)) return null;

    const targetLower = targetModelName.toLowerCase();
    const matchedModel = group.models.find(
      (m) =>
        m.key.toLowerCase() === targetLower ||
        m.name.toLowerCase() === targetLower ||
        m.variants.some((v) => v.id.toLowerCase() === targetLower || targetLower.includes(v.id.toLowerCase())),
    );
    if (!matchedModel) return null;

    const variant = matchedModel.variants.find(
      (v) => v.id.toLowerCase() === targetLower || targetLower.includes(v.id.toLowerCase()),
    ) ?? matchedModel.variants[0];

    return {
      provider: parsedProviderId,
      model: variant.id,
      effort: variant.effort,
      fast: variant.fast,
      tier: null,
    };
  }

  // No prefix: search model name across runnable providers
  if (targetModelName) {
    const targetLower = targetModelName.toLowerCase();
    for (const group of groups) {
      if (!runnable(group.provider)) continue;
      for (const m of group.models) {
        if (
          m.key.toLowerCase() === targetLower ||
          m.name.toLowerCase() === targetLower ||
          m.variants.some((v) => v.id.toLowerCase() === targetLower)
        ) {
          const variant = m.variants.find((v) => v.id.toLowerCase() === targetLower) ?? m.variants[0];
          return {
            provider: group.provider.id,
            model: variant.id,
            effort: variant.effort,
            fast: variant.fast,
            tier: null,
          };
        }
      }
    }
  }

  return null;
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
  const { providerEnabled, providerTrust, blockedModels } = useSettings.getState();

  const resolved = resolveTaskProviderAndModel(
    task.assigned_model,
    providers,
    providerEnabled,
    providerTrust,
    blockedModels,
  );

  if (!resolved) {
    const reason = `[系统错误] 无法为任务 ${task.id} 解析可运行的 CLI/模型（assigned_model=${task.assigned_model ?? "null"}）`;
    executionStore.updateTaskState(sessionId, task.id, "BLOCKED", reason);
    throw new Error(reason);
  }

  // 2. Sync claim before any await (F1.c): READY/REPAIRING → RUNNING + register ctx
  const predecessorId =
    executionStore.pipelines[sessionId]?.taskTurnIds?.[task.id] ?? null;
  const turnId = `subagent-${sessionId}-${task.id}-${Date.now().toString(36)}`;
  const claimLog = `[TaskRunner] 启动子代理进程 (Turn: ${turnId})，分配 CLI: ${resolved.provider}，模型: ${resolved.model}`;
  const claimed = executionStore.claimTaskRunning(sessionId, task.id, turnId, claimLog);
  if (!claimed) {
    throw new Error(`任务 ${task.id} 已被认领或状态不可派发`);
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
            `[窗口复用] 续接任务 ${reusable.taskId}（${reusable.decision.reason}；节省 ${(reusable.decision.savingPercentage * 100).toFixed(0)}%）`,
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
          `工作区(复用): ${taskCwd}`,
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
            `工作区: ${taskCwd} (${wt.branch} @ ${baseCommit.slice(0, 8) || "?"})`,
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
      `[系统错误] 调起子代理进程异常: ${err}`,
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
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, `[模型指派] ${e.name}`);
      break;

    case "text":
      if (!isCancellablePipelineState(liveState())) break;
      ctx.rawOutput += e.text;
      break;

    case "thread":
      if (!isCancellablePipelineState(liveState())) break;
      executionStore.setTaskThreadId(ctx.sessionId, ctx.taskId, e.id);
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, `[会话] CLI thread=${e.id}`);
      break;

    case "reasoning": {
      if (!isCancellablePipelineState(liveState())) break;
      const preview = e.text.trim().split("\n")[0]?.slice(0, 120) ?? "";
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        `[思考] ${e.text.length} 字${preview ? `：${preview}` : ""}`,
      );
      break;
    }

    case "quota": {
      if (!isCancellablePipelineState(liveState())) break;
      const reset = e.resetAt ? `，重置 ${e.resetAt}` : "";
      const retry = e.retrying ? "（重试中）" : "";
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        `[额度] ${e.raw}${reset}${retry}`,
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
        `[待审批] ${e.command} (${e.level}: ${e.reason})`,
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
        `[工具调用: ${e.tool}] ${e.title}${detail}`,
      );
      break;
    }

    case "toolEnd": {
      if (!isCancellablePipelineState(liveState())) break;
      const status = e.ok === false ? "✗ 失败" : "✓ 成功";
      const out = e.output ? `: ${e.output.slice(0, 150)}` : "";
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        `[工具完成: ${e.id}] ${status}${out}`,
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
        `[文件变更] ${e.change}: ${e.path} (+${e.additions}, -${e.deletions})`,
      );
      break;
    }

    case "warning":
      if (!isCancellablePipelineState(liveState())) break;
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, `[警告] ${e.message}`);
      break;

    case "error": {
      ctx.sawError = true;
      const st = liveState();
      if (!isCancellablePipelineState(st)) {
        executionStore.appendTaskLog(
          ctx.sessionId,
          ctx.taskId,
          `[忽略] 终态任务迟到 error 事件: ${e.message}`,
        );
        activeSubAgents.delete(e.turnId);
        break;
      }
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, `[错误] ${e.message}`);
      const timeoutReason = formatAgentTimeoutBlockReason(e.message);
      if (timeoutReason) {
        const evidence = [
          `最后输出片段: ${(ctx.rawOutput || "").slice(-500) || "（无）"}`,
          `已改动文件: ${ctx.changedFiles.length ? ctx.changedFiles.join(", ") : "（无）"}`,
        ].join("\n");
        executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, `[超时证据]\n${evidence}`);
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
        `[执行异常] 子代理进程退出: ${e.message}`,
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
          `[忽略] 终态任务迟到 done 事件（当前 ${st ?? "unknown"}）`,
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
          `[等待审批] ${cmds}`,
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
    `[集成工作区] ${wt.branch} @ ${wt.root}`,
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
      committed ? `[提交] 任务 worktree 已提交` : `[提交] 无改动可提交`,
    );

    const outcome = await withSessionMergeLock(sessionId, () =>
      mergeTaskWorktree(integration.root, integration.branch, wt.branch, task),
    );

    if ("success" in outcome) {
      executionStore.appendTaskLog(
        sessionId,
        task.id,
        `[合并] 已合入 ${outcome.success.mergedBranch}`,
      );
      try {
        await removeTaskWorktree(wt.root, wt.branch, true, false);
        executionStore.setTaskWorktree(sessionId, task.id, null);
      } catch (err) {
        executionStore.appendTaskLog(
          sessionId,
          task.id,
          `[清理] 移除任务 worktree 失败（分支已保留）: ${err}`,
        );
      }
      return { ok: true };
    }

    // Conflict: original task DONE; inject integration task; rewrite downstream deps.
    const { conflictedFiles, integrationTask } = outcome.conflict;
    executionStore.appendTaskLog(
      sessionId,
      task.id,
      `[合并冲突] 已生成集成任务 ${integrationTask.id}（冲突文件: ${conflictedFiles.join(", ")}）`,
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
      `[集成任务] 等待合并冲突解决；冲突文件: ${conflictedFiles.join(", ")}`,
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
        `[集成工作区] 预合并失败，派发时将重试: ${err}`,
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
      "[验收通过] 任务未声明验收命令，直接通过关卡",
    );
    return { passed: true, evidence: "", results, aborted: false };
  }

  for (const cmd of commands) {
    if (pipelineTaskState(ctx.sessionId, ctx.taskId) !== "VERIFYING") {
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        "[忽略] 关卡执行中止：任务已离开 VERIFYING（可能已取消）",
      );
      return {
        passed: false,
        evidence: failureOutputs.join("\n---\n"),
        results,
        aborted: true,
      };
    }
    executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, `[验收命令] 执行关卡: ${cmd}`);
    try {
      const envAllow = useSettings.getState().gateEnvAllow ?? [];
      const gateRunId = `gate:${ctx.sessionId}:${ctx.taskId}`;
      const confirmed = ctx.task.acceptance?.confirmedCommands?.includes(cmd) === true;
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        "[验收排队] 等待其他验收命令结束后再执行…",
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
          "[忽略] 关卡结果丢弃：任务已离开 VERIFYING",
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
          `[验收通过] 命令 "${cmd}" 执行成功 (耗时: ${gateRes.durationMs}ms)`,
        );
      } else {
        const failMsg = `[验收失败] 命令 "${cmd}" 未通过 (退出码: ${gateRes.exitCode}):\n${gateRes.outputTail || gateRes.errorMessage || "执行失败"}`;
        executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, failMsg);
        failureOutputs.push(failMsg);
        return {
          passed: false,
          evidence: failureOutputs.join("\n---\n"),
          results,
          aborted: false,
        };
      }
    } catch (cmdErr) {
      const errDetail = `[验收异常] 命令 "${cmd}" 执行抛出异常: ${cmdErr}`;
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, errDetail);
      failureOutputs.push(errDetail);
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
    "[手动接管] 开始重跑验收关卡…",
  );

  const gate = await runGatesForTask({ sessionId, taskId, task, workdir });
  if (gate.aborted) return false;

  if (gate.passed) {
    executionStore.updateTaskState(
      sessionId,
      taskId,
      "DONE",
      "[手动接管完成，验收通过]",
    );
    void recordOutcomeForTask(sessionId, task, "USER_TAKEOVER", pipeline.taskTurnIds?.[taskId] ?? taskId, null);
    await progressDownstreamTasks(sessionId);
    return true;
  }

  executionStore.updateTaskState(
    sessionId,
    taskId,
    "BLOCKED",
    `[任务阻塞] 手动接管后验收未通过: ${gate.evidence.slice(0, 300)}`,
  );
  return false;
}

/** Resolve pasteable resume command for a blocked task (F10.c). */
export async function getTaskTerminalResumeCommand(
  sessionId: string,
  taskId: string,
): Promise<{ command: string | null; hint?: string }> {
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (!pipeline) return { command: null, hint: "无流水线会话" };

  const providerRaw =
    pipeline.taskProviders?.[taskId] ||
    pipeline.assignments?.find((a) => a.taskId === taskId)?.provider ||
    null;
  const threadId = pipeline.threadIds?.[taskId] ?? null;
  const cwd =
    pipeline.taskCwds?.[taskId] || pipeline.baseCwd || pipeline.cwd || ".";

  if (!providerRaw) {
    return { command: null, hint: "无可续接会话（未知 Provider）" };
  }
  if (providerRaw === "AIDER") {
    return {
      command: null,
      hint: `在任务目录运行 aider --restore-chat-history（cwd: ${cwd}）`,
    };
  }
  if (!threadId) {
    return { command: null, hint: "无可续接会话（无 CLI 会话 ID）" };
  }

  try {
    const command = await terminalResumeCommand(
      providerRaw as ProviderId,
      threadId,
      cwd,
    );
    if (!command) {
      return { command: null, hint: "无可续接会话" };
    }
    return { command };
  } catch (err) {
    return { command: null, hint: `生成续接命令失败: ${err}` };
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
      "[忽略] 完成处理中止：任务已不在 RUNNING",
    );
    activeSubAgents.delete(ctx.turnId);
    return;
  }

  // 1. Transition to VERIFYING (m4b)
  executionStore.updateTaskState(
    ctx.sessionId,
    ctx.taskId,
    "VERIFYING",
    "[验收关卡] 子代理编码轮次结束，开始执行自动化验收关卡...",
  );

  // 1b. R6 write-scope check before any gate command (F5.i)
  executionStore.recordTaskFailureEvidence(ctx.sessionId, ctx.taskId, null);
  try {
    const outOfScopeFiles = await findOutOfScopeFiles(ctx);
    if (pipelineTaskState(ctx.sessionId, ctx.taskId) !== "VERIFYING") {
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        "[忽略] 写入范围校验结果丢弃：任务已离开 VERIFYING",
      );
      activeSubAgents.delete(ctx.turnId);
      return;
    }
    if (outOfScopeFiles.length > 0) {
      const listing = outOfScopeFiles.join(", ");
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        `[写入越界] 以下文件超出 write_scope (${ctx.task.write_scope.join(", ") || "无"})，已跳过验收关卡:\n${outOfScopeFiles.join("\n")}`,
      );
      // F13.c / R6: same repair ladder as gate failure
      await applyRepairLadder(ctx, {
        failedCommand: null,
        outputTail: `写入范围越界: ${listing}`,
        outOfScopeFiles,
        previousChangedFiles: [...ctx.changedFiles],
      });
      return;
    }
  } catch (scopeErr) {
    console.warn("[TaskRunner] 写入范围校验异常，继续执行验收关卡:", scopeErr);
    executionStore.appendTaskLog(
      ctx.sessionId,
      ctx.taskId,
      `[写入范围校验异常] ${scopeErr}，继续执行验收关卡`,
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
      "[忽略] 完成收尾中止：任务已离开 VERIFYING",
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
        ? `共 ${commands.length} 项关卡命令全部验收通过`
        : "无额外关卡命令";

    const changedFilesStat =
      ctx.changedFiles.length > 0
        ? `${ctx.changedFiles.length} 个文件被修改/添加`
        : "未捕获到文件变动";

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
      console.warn("[TaskRunner] 解析任务报告异常，回退到本地默认报告:", repErr);
      report = {
        taskId: ctx.taskId,
        title: ctx.task.title,
        model: ctx.task.assigned_model || "unknown",
        attemptId: ctx.turnId,
        firstPass: true,
        isLocallyGenerated: true,
        completedWork: ctx.rawOutput || "完成子代理任务执行",
        tipsForDownstream: "无",
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
          `[任务阻塞] worktree 提交/合并失败: ${wtResult.error.slice(0, 300)}`,
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
      `[任务完成] 验收通过，已交付产物${snippet ? `: ${snippet}` : ""}`,
    );
    void recordAttemptEnd(ctx.turnId, "DONE");
    void recordOutcomeForTask(
      ctx.sessionId,
      ctx.task,
      outcomeResultForCompletion(repairStep),
      ctx.turnId,
      report.costUsd ?? doneEvent?.costUsd ?? null,
    );

    // F9.e: non-empty contractDeviations (not 「无」) pause transitive downstream
    const deviation = (report.contractDeviations ?? "").trim();
    if (deviation && deviation !== "无") {
      executionStore.addDeviationSource(ctx.sessionId, ctx.taskId);
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        `[契约偏离] ${deviation.slice(0, 200)} — 下游已暂停，待确认后继续`,
      );
    }

    activeSubAgents.delete(ctx.turnId);
    syncMainSessionState(ctx.sessionId);
    await maybeCheckPlanBudget(ctx.sessionId);

    // Progress DAG to advance downstream tasks
    await progressDownstreamTasks(ctx.sessionId);
  } else {
    const failureEvidence = failureOutputs.join("\n---\n") || "验收命令非零退出";
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
    append("[交付] 计划已完成，开始全量关卡…");

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
      append(`[交付关卡] ${cmd}`);
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
            `[交付关卡失败] "${cmd}" 退出码 ${gateRes.exitCode}: ${(gateRes.outputTail || "").slice(0, 200)}`,
          );
          break;
        }
        append(`[交付关卡通过] ${cmd}`);
      } catch (err) {
        fullGatePassed = false;
        append(`[交付关卡异常] ${cmd}: ${err}`);
        break;
      }
    }

    if (commands.length === 0) {
      append("[交付] 无验收命令，跳过全量关卡");
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
          goal: pipeline.tasks[0]?.goal || "计划交付",
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
      error: fullGatePassed ? null : "全量关卡未全部通过；仍可导出补丁或保留分支",
    });
    append(
      fullGatePassed
        ? `[交付就绪] 改动 ${summary.touchedFiles.length} 个文件，成本 $${summary.actualCostUsd.toFixed(2)}`
        : `[交付就绪] 全量关卡未通过 — 可导出补丁或保留分支`,
    );

    // F14.g: settle assignment savings when plan reaches terminal delivery
    if (pipeline.planDbId != null) {
      try {
        const refModel = useSettings.getState().referenceModel;
        const override =
          refModel && refModel !== "auto" ? refModel : undefined;
        const eventId = await settlePlanSavings(pipeline.planDbId, override);
        if (eventId != null) {
          append(`[节省结算] 已写入 savings_events (#${eventId})`);
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
    append(`[交付失败] ${err}`);
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
      store.appendTaskLog(sessionId, logTaskId, `[交付] 清理集成 worktree: ${err}`);
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
      `[交付] 已合并 ${pipeline.integration.branch} 到当前检出`,
    );
    return true;
  } catch (err) {
    const code = String(err);
    let msg = code;
    if (code.includes("DIRTY")) {
      msg = "主检出有未提交改动，无法合并。请先提交/清理，或改用「导出补丁」。";
    } else if (code.includes("CONFLICT")) {
      msg = "合并冲突，已中止。请改用「导出补丁」。";
    }
    store.setDelivery(sessionId, {
      ...(pipeline.delivery ?? { phase: "ready" }),
      phase: "ready",
      error: msg,
    });
    store.appendTaskLog(sessionId, logTaskId, `[交付合并失败] ${msg}`);
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
    store.appendTaskLog(sessionId, logTaskId, `[交付] 已导出补丁: ${outPath}`);
    return outPath;
  } catch (err) {
    store.setDelivery(sessionId, {
      ...(pipeline.delivery ?? { phase: "ready" }),
      phase: "ready",
      error: `导出补丁失败: ${err}`,
    });
    store.appendTaskLog(sessionId, logTaskId, `[交付导出失败] ${err}`);
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
    store.appendTaskLog(sessionId, logTaskId, `[交付] 清理 worktree: ${err}`);
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
    `[交付] 已保留分支 ${branch}，worktree 目录已清理`,
  );
}

/** Human-readable wait reason for READY tasks blocked by the scheduler (F9.d). */
export function formatTaskWaitReason(reason: DispatchIneligibleReason): string {
  if ("globalSlotsFull" in reason) {
    const { current, max } = reason.globalSlotsFull;
    return `等待：全局并发已满 ${current}/${max}`;
  }
  if ("providerSlotsFull" in reason) {
    const { provider, current, max } = reason.providerSlotsFull;
    return `等待：${provider} 并发已满 ${current}/${max}`;
  }
  if ("writeScopeCollision" in reason) {
    const { conflictingTaskId } = reason.writeScopeCollision;
    return `等待：写入范围与 ${conflictingTaskId} 冲突`;
  }
  if ("contractDeviationPaused" in reason) {
    const { sourceTaskId } = reason.contractDeviationPaused;
    return `等待：上游 ${sourceTaskId} 契约偏离待确认`;
  }
  return "等待：调度约束";
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
    console.error("[TaskRunner] pickDispatchableTasks 失败:", err);
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
        `[就绪] 前置依赖已全部完成${deps ? ` (${deps})` : ""}，等待调度执行`,
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
      console.error(`[TaskRunner] 启动任务 ${task.id} 失败:`, err);
      const state = useExecution.getState().pipelines[sessionId]?.taskStates[task.id];
      if (state !== "BLOCKED") {
        executionStore.updateTaskState(
          sessionId,
          task.id,
          "BLOCKED",
          `[系统错误] 调起子代理进程异常: ${err}`,
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
    `[契约偏离] 用户确认继续，已解除下游暂停`,
  );
  return dispatchReadyTasks(sessionId);
}
