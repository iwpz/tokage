import type {
  PlanContract,
  ProviderId,
  ProviderInfo,
  TaskItem,
  UpstreamArtifact,
} from "./ipc";
import { assembleTaskBrief, createSessionWorktree, startChatTurn } from "./ipc";
import { runnable, selectableGroups } from "./models";
import { useExecution } from "../stores/execution";
import { useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";

export interface ResolvedModel {
  provider: ProviderId;
  model: string;
  effort: string | null;
  fast: boolean;
  tier: string | null;
  fallbackUsed?: boolean;
}

/**
 * Maps an assigned model string (e.g. "CLAUDE_CODE:claude-3-7-sonnet", "openai:gpt-4o", "claude-3-7-sonnet")
 * to a concrete runnable ProviderId and CLI model parameters.
 * When the specified CLI is unavailable/melted/unsupported, falls back to an available runnable CLI.
 */
export function resolveTaskProviderAndModel(
  assignedModelSpec: string | null | undefined,
  providers: ProviderInfo[],
  enabled: Partial<Record<ProviderId, boolean>> = {},
  trust: Partial<Record<ProviderId, boolean>> = {},
  blockedModels: string[] = [],
): ResolvedModel | null {
  const spec = (assignedModelSpec || "").trim();

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

  // Filter available and runnable providers
  const groups = selectableGroups(providers, enabled, trust, "direct", blockedModels);

  // Check if target provider is available
  if (parsedProviderId) {
    const group = groups.find((g) => g.provider.id === parsedProviderId);
    if (group && runnable(group.provider)) {
      // Find matching model or variant
      const targetLower = targetModelName.toLowerCase();
      const matchedModel = group.models.find(
        (m) =>
          m.key.toLowerCase() === targetLower ||
          m.name.toLowerCase() === targetLower ||
          m.variants.some((v) => v.id.toLowerCase() === targetLower || targetLower.includes(v.id.toLowerCase())),
      );

      if (matchedModel) {
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

      // If specific model didn't match directly in this provider, use the provider's default/first model
      if (group.models.length > 0) {
        const first = group.models[0];
        const v = first.variants[0];
        return {
          provider: parsedProviderId,
          model: v?.id ?? first.key,
          effort: v?.effort ?? null,
          fast: v?.fast ?? false,
          tier: null,
        };
      }
    }
  }

  // 2. No explicit provider or target provider not runnable: search across all available providers for model name match
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

  // 3. Fallback: pick the first runnable provider and its best model
  const fallbackGroup = groups.find((g) => runnable(g.provider) && g.models.length > 0);
  if (fallbackGroup) {
    const firstModel = fallbackGroup.models[0];
    const variant = firstModel.variants[0];
    return {
      provider: fallbackGroup.provider.id,
      model: variant?.id ?? firstModel.key,
      effort: variant?.effort ?? null,
      fast: variant?.fast ?? false,
      tier: null,
      fallbackUsed: true,
    };
  }

  // Fallback to CLAUDE_CODE or CODEX_CLI default if no providers detected (e.g. mock/test environment)
  return {
    provider: (parsedProviderId ?? "CLAUDE_CODE") as ProviderId,
    model: targetModelName || "claude-3-7-sonnet",
    effort: null,
    fast: false,
    tier: null,
    fallbackUsed: true,
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
 * model resolution, workspace isolation, infinite timeout, and state binding.
 */
export async function dispatchSubAgentTask(
  options: TaskRunnerOptions,
): Promise<{ turnId: string; worktreeCwd: string; resolved: ResolvedModel }> {
  const { sessionId, task, contracts = [], upstreamArtifacts = [], failureEvidence, baseCwd, useWorktree } = options;

  // 1. Resolve Provider and Model
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
    throw new Error(`无法为任务 ${task.id} 找到可用的 CLI 执行器。`);
  }

  // 2. Isolate workspace if requested (Git worktree per task or session)
  let taskCwd = baseCwd;
  if (useWorktree) {
    try {
      const wtName = `task-${sessionId}-${task.id}-${Date.now().toString(36)}`;
      const wt = await createSessionWorktree(baseCwd, wtName);
      if (wt?.cwd) {
        taskCwd = wt.cwd;
      }
    } catch (e) {
      console.warn(`[TaskRunner] 创建独立 Worktree 失败，回退到主工作目录: ${e}`);
    }
  }

  // 3. Assemble Task Brief prompt
  const prompt = await buildSubAgentPrompt(task, contracts, upstreamArtifacts, failureEvidence);

  // 4. Generate unique turnId
  const turnId = `subagent-${sessionId}-${task.id}-${Date.now().toString(36)}`;

  // 5. Update execution store
  const executionStore = useExecution.getState();
  executionStore.updateTaskState(
    sessionId,
    task.id,
    "RUNNING",
    `[TaskRunner] 启动子代理进程 (Turn: ${turnId})，分配 CLI: ${resolved.provider}，模型: ${resolved.model}` +
      (resolved.fallbackUsed ? " (使用可用备选 CLI 回退)" : "") +
      (taskCwd !== baseCwd ? `，工作区: ${taskCwd}` : ""),
    turnId,
  );

  // 6. Find project trust
  const project = useProjects.getState().projects.find((p) => p.sessions.some((s) => s.id === sessionId));
  const trustedList: ProviderId[] =
    project?.trusted === true
      ? (Object.entries(useSettings.getState().providerTrust)
          .filter(([, v]) => v)
          .map(([k]) => k as ProviderId))
      : [resolved.provider];

  // 7. Invoke startChatTurn with INFINITE timeout
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
    timeoutTier: "INFINITE",
    timeoutMultiplier: useSettings.getState().timeoutMultiplier ?? 1.0,
  });

  return {
    turnId,
    worktreeCwd: taskCwd,
    resolved,
  };
}

/**
 * Dispatches all eligible ready tasks adhering to concurrency limits and isolation configuration.
 */
export async function dispatchReadyTasks(sessionId: string): Promise<string[]> {
  const executionStore = useExecution.getState();
  const pipeline = executionStore.pipelines[sessionId];
  if (!pipeline || pipeline.status === "PAUSED" || pipeline.status === "DONE") {
    return [];
  }

  const { maxParallel, isolation } = useSettings.getState();
  const maxConcurrency = Math.max(1, maxParallel || 3);
  const useWorktree = isolation === "worktree";

  // Count currently running tasks
  const runningTaskCount = pipeline.tasks.filter(
    (t) => pipeline.taskStates[t.id] === "RUNNING",
  ).length;

  const availableSlots = maxConcurrency - runningTaskCount;
  if (availableSlots <= 0) {
    return [];
  }

  // Find candidate tasks:
  // Either marked "READY" or "PENDING" whose dependencies are all "DONE"
  const readyCandidates = pipeline.tasks.filter((t) => {
    const state = pipeline.taskStates[t.id];
    if (state === "RUNNING" || state === "DONE" || state === "BLOCKED" || state === "CANCELLED") {
      return false;
    }
    const allDepsDone = t.depends_on.every((depId) => pipeline.taskStates[depId] === "DONE");
    return allDepsDone;
  });

  const tasksToLaunch = readyCandidates.slice(0, availableSlots);
  const launchedTurnIds: string[] = [];

  const baseCwd = pipeline.cwd || ".";
  const contracts = pipeline.contracts || [];
  const upstreamArtifactsList: UpstreamArtifact[] = Object.values(pipeline.upstreamArtifacts || {});

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
      executionStore.updateTaskState(
        sessionId,
        task.id,
        "BLOCKED",
        `[系统错误] 调起子代理进程异常: ${err}`,
      );
    }
  }

  return launchedTurnIds;
}
