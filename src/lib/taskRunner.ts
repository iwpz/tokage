import type {
  ChatEvent,
  DispatchIneligibleReason,
  PipelineGateResult,
  PlanContract,
  ProviderId,
  ProviderInfo,
  TaskItem,
  TaskReport,
  UpstreamArtifact,
} from "./ipc";
import {
  assembleTaskBrief,
  cancelChatTurn,
  commitTaskWorktree,
  createSessionWorktree,
  cancelGateCommand,
  executeGateCommand,
  gitRevParse,
  gitStagedDiffNames,
  mergeTaskWorktree,
  onChatEnded,
  onChatEvent,
  parseOrGenerateTaskReport,
  pickDispatchableTasks,
  prepareIntegrationWorktree,
  recordTaskReport,
  removeTaskWorktree,
  resolveApproval,
  startChatTurn,
  terminalResumeCommand,
  validateWriteScope,
  type ChatEndedEvent,
  type UserDecision,
} from "./ipc";
import type { TaskState } from "./ipc";
import { runnable, selectableGroups } from "./models";
import { useExecution, type TaskApproval } from "../stores/execution";
import { useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";

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

  const prompt =
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

  try {
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
      role: "EXECUTOR",
      writeScope: task.write_scope,
      timeoutTier: "AGENT",
      idleTimeoutSecs: Math.round((useSettings.getState().agentIdleTimeoutMin ?? 30) * 60),
      hardTimeoutSecs: Math.round((useSettings.getState().agentHardTimeoutMin ?? 120) * 60),
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

  try {
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
      role: "EXECUTOR",
      writeScope: task.write_scope,
      timeoutTier: "AGENT",
      idleTimeoutSecs: Math.round((useSettings.getState().agentIdleTimeoutMin ?? 30) * 60),
      hardTimeoutSecs: Math.round((useSettings.getState().agentHardTimeoutMin ?? 120) * 60),
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

  // 2. Sync claim before any await (F1.c): READY/PENDING → RUNNING + register ctx
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
    // 3. Isolate workspace if requested (F11: from integration branch + record baseCommit)
    let taskCwd = baseCwd;
    if (useWorktree) {
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
    }
    // F7: persist task cwd + provider for crash recovery / continue.
    executionStore.setTaskCwd(sessionId, task.id, taskCwd);
    executionStore.setTaskProvider(sessionId, task.id, resolved.provider);

    // 4. Assemble Task Brief prompt
    const prompt = await buildSubAgentPrompt(task, contracts, upstreamArtifacts, failureEvidence);

    // 5. Project trust
    const project = useProjects.getState().projects.find((p) => p.sessions.some((s) => s.id === sessionId));
    const trustedList: ProviderId[] =
      project?.trusted === true
        ? (Object.entries(useSettings.getState().providerTrust)
            .filter(([, v]) => v)
            .map(([k]) => k as ProviderId))
        : [resolved.provider];

    // 6. Invoke startChatTurn
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
      resume: null,
      fork: false,
      trusted: trustedList,
      sessionId,
      projectId: project?.id ?? null,
      role: "EXECUTOR",
      writeScope: task.write_scope,
      timeoutTier: "AGENT",
      idleTimeoutSecs: Math.round((useSettings.getState().agentIdleTimeoutMin ?? 30) * 60),
      hardTimeoutSecs: Math.round((useSettings.getState().agentHardTimeoutMin ?? 120) * 60),
    });

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
      const approval: TaskApproval = {
        id: e.id,
        turnId: e.turnId,
        toolId: e.toolId ?? null,
        command: e.command,
        level: e.level,
        reason: e.reason,
        recoverable: e.recoverable ?? null,
        resolved: false,
      };
      executionStore.upsertTaskApproval(ctx.sessionId, ctx.taskId, approval);
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        `[待审批] ${e.command} (${e.level}: ${e.reason})`,
      );
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
      const blockReason =
        timeoutReason ?? `[执行异常] 子代理进程退出: ${e.message}`;
      if (timeoutReason) {
        const evidence = [
          `最后输出片段: ${(ctx.rawOutput || "").slice(-500) || "（无）"}`,
          `已改动文件: ${ctx.changedFiles.length ? ctx.changedFiles.join(", ") : "（无）"}`,
        ].join("\n");
        executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, `[超时证据]\n${evidence}`);
      }
      executionStore.updateTaskState(ctx.sessionId, ctx.taskId, "BLOCKED", blockReason);
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
  const wt = await createSessionWorktree(projectPath, name, null);
  executionStore.setIntegration(sessionId, {
    branch: wt.branch,
    root: wt.root,
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
      executionStore.recordTaskFailureEvidence(ctx.sessionId, ctx.taskId, {
        failedCommand: null,
        outputTail: `写入范围越界: ${listing}`,
        outOfScopeFiles,
        previousChangedFiles: [...ctx.changedFiles],
        workdir: ctx.worktreeCwd,
      });
      executionStore.appendTaskLog(
        ctx.sessionId,
        ctx.taskId,
        `[写入越界] 以下文件超出 write_scope (${ctx.task.write_scope.join(", ") || "无"})，已跳过验收关卡:\n${outOfScopeFiles.join("\n")}`,
      );
      executionStore.updateTaskState(
        ctx.sessionId,
        ctx.taskId,
        "BLOCKED",
        `[任务阻塞] 写入范围越界 (${outOfScopeFiles.length} 个文件): ${listing.slice(0, 300)}`,
      );
      activeSubAgents.delete(ctx.turnId);
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
      };
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
        return;
      }
    }

    // Mark task DONE
    const snippet = (report.completedWork || "").slice(0, 100);
    executionStore.updateTaskState(
      ctx.sessionId,
      ctx.taskId,
      "DONE",
      `[任务完成] 验收通过，已交付产物${snippet ? `: ${snippet}` : ""}`,
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

    // Progress DAG to advance downstream tasks
    await progressDownstreamTasks(ctx.sessionId);
  } else {
    const failureEvidence = failureOutputs.join("\n---\n") || "验收命令非零退出";
    executionStore.updateTaskState(
      ctx.sessionId,
      ctx.taskId,
      "BLOCKED",
      `[任务阻塞] 自动化验收关卡未通过: ${failureEvidence.slice(0, 300)}`,
    );
    activeSubAgents.delete(ctx.turnId);
  }
}

/**
 * Advances the DAG after a task completes (F9: promotion happens inside `dispatchReadyTasks`).
 */
export async function progressDownstreamTasks(sessionId: string): Promise<string[]> {
  return dispatchReadyTasks(sessionId);
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
