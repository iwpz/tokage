import { demoConfig } from "./demoIdentity";
import { useSettings } from "../stores/settings";
import { debugTarget, type DebugProviderConfig, type DebugRoute } from "./debugProviderTypes";
import type { AttemptRow, ChatRequest, ProviderInfo } from "./ipc";
import builtinText from "../../src-tauri/resources/knowledge/builtin.json?raw";
const builtin: unknown = JSON.parse(builtinText);

/** Snapshots outlive turns: completion/gates can arrive after settings have changed. */
const turns = new Map<string, DebugProviderConfig>();
const routes = new Map<string, DebugRoute>();
const sessions = new Map<string, DebugProviderConfig>();

export function debugConfig(): DebugProviderConfig { return { ...useSettings.getState().debugProvider }; }

export function rememberDebugTurn(request: ChatRequest, config: DebugProviderConfig): void {
  turns.set(request.turnId, config);
  if (request.sessionId) sessions.set(request.sessionId, config);
}

export function rememberDebugRoute(turnId: string, route: DebugRoute): void { routes.set(turnId, route); }
export function debugTurnConfig(turnId?: string | null): DebugProviderConfig | undefined { return turnId ? turns.get(turnId) : undefined; }
export function debugSessionConfig(sessionId: string): DebugProviderConfig | undefined { return demoConfig(sessionId, useSettings.getState().demoChunkDelayMs) ?? sessions.get(sessionId); }

export function plannedDebugRoute(provider: string, model: string, workdir: string, config: DebugProviderConfig): DebugRoute | undefined {
  if (config.output === "off") return undefined;
  const target = debugTarget(config);
  return { output: config.output, requestedProvider: provider, requestedModel: model, actualProvider: target?.provider ?? "DEBUG_PROVIDER", actualModel: target?.model ?? "mock", workdir };
}

export function debugAttempt(att: AttemptRow): AttemptRow {
  att = { ...att, execMode: att.execMode === "PTY" ? "PTY" : "HEADLESS", tokenSource: att.tokenSource === "EXACT" ? "EXACT" : "ESTIMATED" };
  if (!att.mainSessionId) return att; // End-only upserts must not invent metadata.
  const config = demoConfig(att.mainSessionId, useSettings.getState().demoChunkDelayMs) ?? debugTurnConfig(att.id) ?? debugConfig();
  if (config.output === "off") return att;
  const target = debugTarget(config);
  const route = routes.get(att.id);
  return { ...att, debugOutput: config.output, requestedProvider: att.providerName, requestedModel: att.modelId, providerName: route?.actualProvider ?? target?.provider ?? "DEBUG_PROVIDER", modelId: route?.actualModel ?? target?.model ?? "mock" };
}

/** Local catalog snapshot makes all knowledge-pack offerings available immediately. */
export function builtinDebugCatalog(): ProviderInfo[] {
  const pack = builtin as unknown as {
    providers: Record<string, { models?: { model: string; cli_model_arg: string }[] }>;
    models: Record<string, { display_name: string }>;
  };
  return Object.entries(pack.providers).filter(([, p]) => p.models?.length).map(([id, provider]) => {
    const models = [...new Map((provider.models ?? []).map((m) => [m.cli_model_arg, { id: m.cli_model_arg, name: pack.models[m.model]?.display_name ?? m.model }])).values()];
    return {
      id: id as ProviderInfo["id"], name: id.replaceAll("_", " "), executable: "debug", featured: true,
      status: "READY", version: null, path: null, authMethod: "debug", models,
      modelGroups: models.map((m) => ({ key: m.id, name: m.name, efforts: [], defaultEffort: null, contexts: [], thinking: false, fast: false, tags: ["debug"], tiers: [], defaultTier: null, variants: [{ id: m.id, effort: null, context: null, thinking: false, fast: false }] })),
      modelsSource: "BUILTIN", account: null, guidance: [], installMethods: [], trustArgs: [], chat: true,
      adapter: { caps: ["STRUCTURED_OUTPUT", "RESUME", "MODEL_SELECT", "READ_ONLY_MODE"], versions: "debug", tested: "debug" },
      directLimit: null, debugVirtual: true,
    } satisfies ProviderInfo;
  });
}

export function debugCatalogOverlay(real: ProviderInfo[], catalog: ProviderInfo[], config: DebugProviderConfig): ProviderInfo[] {
  if (config.output === "off") return real;
  const merged = new Map(catalog.map((p) => [p.id, p]));
  for (const p of real) {
    const virtual = merged.get(p.id);
    if (!virtual && !p.models?.length) continue;
    const models = [...new Map([...(virtual?.models ?? []), ...(p.models ?? [])].map((m) => [m.id, m])).values()];
    const modelGroups = [...(p.modelGroups ?? []), ...(virtual?.modelGroups ?? []).filter((m) => !(p.modelGroups ?? []).some((g) => g.key === m.key || g.variants.some((v) => v.id === m.key)))];
    merged.set(p.id, { ...virtual, ...p, models, modelGroups, status: "READY", chat: true, directLimit: null, debugVirtual: true });
  }
  return [...merged.values()];
}

/** Only repository/context operations are redirected. Database and app settings stay real. */
export const MOCK_WORKSPACE_COMMANDS = new Set([
  "git_scope_snapshot", "git_info", "git_list_branches", "git_checkout_branch", "git_create_branch", "git_status_files", "git_file_diff",
  "create_session_worktree", "remove_session_worktree", "commit_task_worktree", "merge_task_worktree_cmd",
  "prepare_integration_worktree", "remove_task_worktree", "git_rev_parse", "merge_integration_to_checkout",
  "export_integration_patch", "prune_project_worktrees", "get_repo_digest", "save_session_plan", "persist_plan", "persist_plan_user_edited",
  "reset_plan_to_default", "load_plan", "save_pipeline_state_cmd", "load_pipeline_state_cmd", "tokage_dir_exists", "ignore_tokage_dir",
  "remove_tokage_dir", "execute_gate_command_cmd", "revert_out_of_scope_files_cmd",
]);
