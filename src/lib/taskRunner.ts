import type {
  ChatEvent,
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
  createSessionWorktree,
  cancelGateCommand,
  executeGateCommand,
  onChatEvent,
  parseOrGenerateTaskReport,
  recordTaskReport,
  startChatTurn,
  validateWriteScope,
} from "./ipc";
import type { TaskState } from "./ipc";
import { runnable, selectableGroups } from "./models";
import { useExecution } from "../stores/execution";
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

export async function ensureSubAgentEventListener() {
  if (subAgentListenerCleanup) return;
  subAgentListenerCleanup = await onChatEvent(handleSubAgentChatEvent);
}

export function resetSubAgentEventListener() {
  if (subAgentListenerCleanup) {
    subAgentListenerCleanup();
    subAgentListenerCleanup = null;
  }
  activeSubAgents.clear();
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
    // 3. Isolate workspace if requested
    let taskCwd = baseCwd;
    if (useWorktree) {
      const wtName = `task-${sessionId}-${task.id}-${Date.now().toString(36)}`;
      const wt = await createSessionWorktree(baseCwd, wtName);
      if (wt?.cwd) {
        taskCwd = wt.cwd;
        const ctx = getRunningSubAgent(turnId);
        if (ctx) ctx.worktreeCwd = taskCwd;
        useExecution.getState().appendTaskLog(sessionId, task.id, `工作区: ${taskCwd}`);
      }
    }

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
              ctx = {
                sessionId: session.sessionId,
                taskId,
                task,
                worktreeCwd: session.cwd || ".",
                turnId: e.turnId,
                rawOutput: "",
                changedFiles: [],
                toolCalls: [],
                startTime: session.taskStartedAt?.[taskId] || Date.now(),
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
async function findOutOfScopeFiles(ctx: RunningSubAgentContext): Promise<string[]> {
  if (ctx.changedFiles.length === 0) return [];
  const scopeRes = await validateWriteScope(ctx.task.write_scope ?? [], ctx.changedFiles);
  let outOfScope = scopeRes.outOfScopeFiles;
  if (outOfScope.length === 0 || useSettings.getState().isolation !== "inplace") {
    return outOfScope;
  }

  const pipeline = useExecution.getState().pipelines[ctx.sessionId];
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

  // 2. Execute acceptance gate commands (m4b)
  const commands = ctx.task.acceptance?.commands ?? [];
  let allPassed = true;
  const failureOutputs: string[] = [];

  if (commands.length > 0) {
    for (const cmd of commands) {
      if (pipelineTaskState(ctx.sessionId, ctx.taskId) !== "VERIFYING") {
        executionStore.appendTaskLog(
          ctx.sessionId,
          ctx.taskId,
          "[忽略] 关卡执行中止：任务已离开 VERIFYING（可能已取消）",
        );
        activeSubAgents.delete(ctx.turnId);
        return;
      }
      executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, `[验收命令] 执行关卡: ${cmd}`);
      try {
        const envAllow = useSettings.getState().gateEnvAllow ?? [];
        const gateRunId = `gate:${ctx.sessionId}:${ctx.taskId}`;
        const confirmed =
          ctx.task.acceptance?.confirmedCommands?.includes(cmd) === true;
        executionStore.appendTaskLog(
          ctx.sessionId,
          ctx.taskId,
          "[验收排队] 等待其他验收命令结束后再执行…",
        );
        const gateRes = await executeGateCommand(
          cmd,
          ctx.worktreeCwd,
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
          activeSubAgents.delete(ctx.turnId);
          return;
        }
        const gateResult: PipelineGateResult = {
          taskId: ctx.taskId,
          command: cmd,
          passed: gateRes.passed,
          exitCode: gateRes.exitCode,
          durationMs: gateRes.durationMs,
          outputTail: gateRes.outputTail || "",
        };
        executionStore.recordGateResult(ctx.sessionId, gateResult);

        if (gateRes.passed) {
          executionStore.appendTaskLog(
            ctx.sessionId,
            ctx.taskId,
            `[验收通过] 命令 "${cmd}" 执行成功 (耗时: ${gateRes.durationMs}ms)`,
          );
        } else {
          allPassed = false;
          const failMsg = `[验收失败] 命令 "${cmd}" 未通过 (退出码: ${gateRes.exitCode}):\n${gateRes.outputTail || gateRes.errorMessage || "执行失败"}`;
          executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, failMsg);
          failureOutputs.push(failMsg);
          break;
        }
      } catch (cmdErr) {
        allPassed = false;
        const errDetail = `[验收异常] 命令 "${cmd}" 执行抛出异常: ${cmdErr}`;
        executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, errDetail);
        failureOutputs.push(errDetail);
        break;
      }
    }
  } else {
    executionStore.appendTaskLog(ctx.sessionId, ctx.taskId, "[验收通过] 任务未声明验收命令，直接通过关卡");
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

    // Mark task DONE
    const snippet = (report.completedWork || "").slice(0, 100);
    executionStore.updateTaskState(
      ctx.sessionId,
      ctx.taskId,
      "DONE",
      `[任务完成] 验收通过，已交付产物${snippet ? `: ${snippet}` : ""}`,
    );

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
 * Calculates dependent downstream tasks whose prerequisites are now satisfied,
 * advances them from PENDING to READY, and launches them (m4c).
 */
export async function progressDownstreamTasks(sessionId: string): Promise<string[]> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline || pipeline.status === "PAUSED" || pipeline.status === "DONE") {
    return [];
  }

  pipeline.tasks.forEach((task) => {
    const currentState = pipeline.taskStates[task.id];
    if (currentState === "PENDING") {
      const allDepsDone = task.depends_on.every((depId) => pipeline.taskStates[depId] === "DONE");
      if (allDepsDone) {
        executionStore.updateTaskState(
          sessionId,
          task.id,
          "READY",
          `[就绪] 前置依赖已全部完成 (${task.depends_on.join(", ")})，等待调度执行`,
        );
      }
    }
  });

  return dispatchReadyTasks(sessionId);
}

/**
 * Single dispatch entry (F1.h): pause check, concurrency, claim/failure handling.
 * `opts.only` limits launch to specific task ids (manual retry / continue).
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

  // Count currently running or verifying tasks
  const runningTaskCount = pipeline.tasks.filter((t) => {
    const state = pipeline.taskStates[t.id];
    return state === "RUNNING" || state === "VERIFYING";
  }).length;

  const availableSlots = maxConcurrency - runningTaskCount;
  if (availableSlots <= 0) {
    return [];
  }

  // Promote PENDING → READY when deps are done, then pick READY candidates
  for (const t of pipeline.tasks) {
    if (opts?.only && !opts.only.includes(t.id)) continue;
    const state = pipeline.taskStates[t.id];
    if (state !== "PENDING") continue;
    const allDepsDone = t.depends_on.every((depId) => pipeline.taskStates[depId] === "DONE");
    if (allDepsDone) {
      executionStore.updateTaskState(sessionId, t.id, "READY", "[就绪] 依赖已满足，等待调度");
    }
  }

  const fresh = useExecution.getState().pipelines[sessionId];
  if (!fresh) return [];

  let readyCandidates = fresh.tasks.filter((t) => fresh.taskStates[t.id] === "READY");
  if (opts?.only?.length) {
    const only = new Set(opts.only);
    readyCandidates = readyCandidates.filter((t) => only.has(t.id));
  }
  const tasksToLaunch = readyCandidates.slice(0, availableSlots);
  const launchedTurnIds: string[] = [];

  const baseCwd = fresh.cwd || ".";
  const contracts = fresh.contracts || [];
  const upstreamArtifactsList: UpstreamArtifact[] = Object.values(fresh.upstreamArtifacts || {});

  for (const task of tasksToLaunch) {
    try {
      const res = await dispatchSubAgentTask({
        sessionId,
        task,
        contracts,
        upstreamArtifacts: upstreamArtifactsList,
        baseCwd,
        useWorktree,
      });
      launchedTurnIds.push(res.turnId);
    } catch (err) {
      console.error(`[TaskRunner] 启动任务 ${task.id} 失败:`, err);
      // failClaimedSubAgent already BLOCKed + unregistered when claim succeeded
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
