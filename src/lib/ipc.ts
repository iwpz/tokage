import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";
import i18n from "../i18n";
import { REPORT_NONE } from "./reportTokens";
import { debugAttempt, debugConfig, debugTurnConfig, rememberDebugRoute, rememberDebugTurn, MOCK_WORKSPACE_COMMANDS } from "./debugProvider";
import { type DebugProviderConfig, type DebugRoute } from "./debugProviderTypes";
import { registerDemoTurn } from "./demoAudit";
import { demoConfig, demoIdentity } from "./demoIdentity";
import { demoFile, demoFixture, demoPlan, demoReport, demoScript } from "./demoScript";
import { useSettings } from "../stores/settings";
import mockPlanMarkdown from "../../src-tauri/resources/debug/plan.md?raw";

const debugWorkspaces = new Map<string, Promise<string>>();
export async function prepareDebugWorkspace(path: string): Promise<string> {
  if (!inTauri) return path;
  let prepared = debugWorkspaces.get(path);
  if (!prepared) {
    prepared = invoke<string>("prepare_debug_workspace", { path });
    debugWorkspaces.set(path, prepared);
    prepared.catch(() => debugWorkspaces.delete(path));
  }
  return prepared;
}

/** The single IPC boundary covers every repository operation in mock mode. */
async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const config = debugConfig();
  if (inTauri && config.output === "mock" && args && MOCK_WORKSPACE_COMMANDS.has(command)) {
    args = { ...args };
    for (const key of ["cwd", "workdir", "path", "root", "projectPath", "integrationRoot"]) {
      if (typeof args[key] === "string" && args[key]) args[key] = await prepareDebugWorkspace(args[key] as string);
    }
  }
  return tauriInvoke<T>(command, args);
}


// Mirrors docs/01 §8 ProviderStatus. Kept hand-written until specta bindings exist.
export type ProviderStatus =
  | "READY"
  | "NOT_INSTALLED"
  | "NEEDS_LOGIN"
  | "UNSUPPORTED_VERSION"
  | "UNKNOWN_AUTH"
  | "DEGRADED"
  | "MELTED";

/** Provider whitelist; mirrors `ProviderId` in src-tauri/src/providers.rs. */
export type ProviderId =
  | "CLAUDE_CODE"
  | "CODEX_CLI"
  | "CURSOR_CLI"
  | "ANTIGRAVITY_CLI"
  | "COPILOT_CLI"
  | "AIDER"
  | "OPENCODE"
  | "GROK_CLI"
  | "KIRO_CLI"
  | "QWEN_CODE"
  | "KIMI_CODE"
  | "QODER_CLI"
  | "AMP"
  | "CRUSH"
  | "GOOSE"
  | "CLINE"
  | "DROID"
  | "AUGGIE"
  | "MUSE_CODE"
  | "TRAE_CLI"
  | "REASONIX";

export type InstallKind =
  | "SCRIPT"
  | "POWERSHELL"
  | "CMD"
  | "HOMEBREW"
  | "MACPORTS"
  | "NPM"
  | "APT"
  | "DNF"
  | "YUM"
  | "APK"
  | "WINGET"
  | "PIPX"
  | "UV"
  | "SCOOP";

export interface InstallMethod {
  kind: InstallKind;
  command: string;
  recommended: boolean;
  /** Whether the tool the command relies on was found on PATH. */
  available: boolean;
  requires: string | null;
}

export interface ModelInfo {
  id: string;
  name: string;
}

/** One concrete id the CLI accepts, within a merged model group. */
export interface ModelVariant {
  id: string;
  effort: string | null;
  /** Context size the id names ("1m"); null is the model's default. */
  context: string | null;
  thinking: boolean;
  fast: boolean;
}

/** Catalog entries that differ only by effort / thinking / fast, merged for display. */
export interface ModelGroup {
  key: string;
  name: string;
  /** Ascending; "default" marks a variant whose id names no effort. Empty for single-mode models. */
  efforts: string[];
  defaultEffort: string | null;
  /** Ascending, "default" first. Empty when there is only one size. */
  contexts: string[];
  thinking: boolean;
  fast: boolean;
  /** Facts kept out of the name, for the info tooltip: context sizes ("1m"), "no-zdr", or a catalog tag. */
  tags: string[];
  /** Routing profiles for a CLI's own auto model (Copilot `--auto-tier`); empty otherwise. */
  tiers: string[];
  defaultTier: string | null;
  variants: ModelVariant[];
}

export type Billing = "SUBSCRIPTION" | "PAY_AS_YOU_GO" | "UNKNOWN";

export interface UsageWindow {
  windowMinutes: number;
  usedPercent: number;
  /** Unix seconds. */
  resetsAt: number | null;
}

export type UsageSourceType = "SESSION_LOG" | "CLI_OUTPUT" | "STATUSLINE_CACHE";

export interface UsageSnapshot {
  windows: UsageWindow[];
  details?: {
    plan: string;
    resetLabel: string | null;
    rows: { kind: "INCLUDED" | "AUTO" | "API" | "ON_DEMAND"; usedPercent: number | null; value: string }[];
  };
  /** When the CLI recorded the numbers (ISO 8601). */
  observedAt: string | null;
  source: UsageSourceType;
}

/** Identity-bound account usage. Vendor availability is separate from utilization. */
export interface AccountUsageSnapshot {
  available?: boolean;
  schema: number; scope: { provider: string; backend: string; endpoint: string | null; accountLabel: string | null; organization: string | null; workspace: string | null }; identityKey: string; identityGeneration: number; fetchedAt: number; validUntil: number; source: string;
  windows: { id: string; scope: string; period: string; windowMinutes: number | null; usedPercent: number | null; resetsAt: number | null }[];
  quantities: { id: string; unit: string; currency: string | null; used: string | null; limit: string | null; remaining: string | null }[];
  display: UsageSnapshot | null;
}
export interface AccountUsageResult {
  status: "NOT_REQUESTED" | "UNVERIFIED" | "LATEST" | "EXPIRED" | "FAILED" | "DISABLED" | "UNSUPPORTED" | "IDENTITY_UNKNOWN" | "IDENTITY_CHANGED" | "BACKOFF" | "CANCELLED";
  identityKey: string | null; identityGeneration: number | null; stateRevision?: number | null;
  sourceLogin?: { path: string; observedAt: number; status: "READY" | "NEEDS_LOGIN"; authMethod: string | null; account: Account | null } | null;
  latestSuccess: AccountUsageSnapshot | null; fromCache: boolean; attemptedAt: number; nextRetryAt: number | null;
  error: "AUTHENTICATION" | "RATE_LIMITED" | "REDIRECT" | "NETWORK" | "INVALID_DATA" | "TIMEOUT" | "IDENTITY" | "CANCELLED" | "PERSISTENCE" | null;
}
export interface UsageQueryCapability {
  backend?: string | null;
  method: "CLI" | "HTTP" | "NONE";
  state: "IDENTITY_REQUIRED" | "IDENTITY_PENDING" | "ADAPTER_PENDING" | "UNSUPPORTED";
  authentication: string;
}

export interface Account {
  billing: Billing;
  official: boolean;
  /** Custom endpoint without userinfo / query / fragment, token-like path segments masked; or a cloud name. */
  endpoint: string | null;
  plan: string | null;
  /** Live PAYG wallet from the CLI's `/status` (e.g. `¥29.55`), when available. */
  balance: string | null;
  usage: UsageSnapshot | null;
}

/** Where `models` came from: the CLI's own catalog, or aliases it documents. */
export type ModelsSource = "CLI" | "BUILTIN";

export type GuidanceCode =
  | "INSTALL"
  | "LOGIN"
  | "SET_API_KEY"
  | "VERIFY_LOGIN"
  | "MODEL_VERIFICATION_REQUIRED"
  | "MODEL_VERIFICATION_EXPIRED"
  | "MODEL_VERIFICATION_CHANGED"
  | "MODEL_VERIFICATION_REJECTED"
  | "THIRD_PARTY_BACKEND"
  | "NO_MODELS"
  | "TIMEOUT"
  | "UNSUPPORTED_VERSION";

export interface Guidance {
  code: GuidanceCode;
  /** Copyable command; never executed by Tokage. */
  command: string | null;
}

export interface ModelVerification {
  state: "REQUIRED" | "VALID" | "EXPIRED" | "SOURCE_CHANGED" | "REJECTED";
  model: string | null;
  verifiedAt: number | null;
  expiresAt: number | null;
}

export interface ProviderInfo {
  id: ProviderId;
  name: string;
  executable: string;
  /** Shown by default; the rest of the whitelist is added by the user. */
  featured: boolean;
  status: ProviderStatus;
  version: string | null;
  path: string | null;
  authMethod: string | null;
  models: ModelInfo[];
  modelGroups: ModelGroup[];
  modelsSource: ModelsSource | null;
  account: Account | null;
  guidance: Guidance[];
  modelVerification?: ModelVerification | null;
  installMethods: InstallMethod[];
  /** Flags Tokage adds to pass workspace trust (`<workspace>` = project path); empty if none. */
  trustArgs: string[];
  /** Tokage can run conversation turns on this CLI (it has a headless adapter). */
  chat: boolean;
  /** The adapter's capability bits and tested versions; null when `chat` is false. */
  adapter: AdapterInfo | null;
  /** How Direct mode is restricted on this CLI until approvals exist (docs/11 §4.2). */
  directLimit: DirectLimit | null;
  /** Catalog identity whose execution is handled by the Debug Provider. */
  debugVirtual?: boolean;
  usageQuery?: UsageQueryCapability;
}

/**
 * `CODEX_READ_ONLY`: Codex `exec` can't ask before commands, so Direct runs read-only.
 * `CURSOR_UNSAFE_PERMISSIONS`: Cursor's config would run commands unasked; Direct is refused.
 */
export type DirectLimit =
  | { code: "CODEX_READ_ONLY" }
  | { code: "CURSOR_UNSAFE_PERMISSIONS"; detail: string[] };

/** Mirrors `Caps` in src-tauri/src/chat/adapter.rs (docs/01 §5, docs/11 §4.1). */
export type AdapterCap =
  | "STRUCTURED_OUTPUT"
  | "RESUME"
  | "MODEL_SELECT"
  | "READ_ONLY_MODE"
  | "WRITE_SCOPE"
  | "COMPACT_HEADLESS"
  | "COMPACT_PTY"
  | "USAGE_QUERY"
  | "NATIVE_SESSIONS"
  | "NATIVE_FORK"
  | "APPROVAL_CALLBACK"
  | "TOOL_RULES"
  | "DENIAL_EVENT"
  | "SHELL_WRAP"
  | "OS_SANDBOX"
  | "OWN_SANDBOX"
  | "NATIVE_WORKSPACE_SANDBOX";

export interface AdapterInfo {
  caps: AdapterCap[];
  /** Tested version range, e.g. ">= 2.1.288, < 3". */
  versions: string;
  /** Version the adapter's recorded sample comes from. */
  tested: string;
}

export const inTauri = isTauri();

type MockProvider = Omit<ProviderInfo, "modelGroups" | "account" | "trustArgs" | "chat" | "adapter" | "directLimit"> &
  Partial<Pick<ProviderInfo, "modelGroups" | "account" | "trustArgs">>;

/** Mirrors the adapter registry in src-tauri/src/chat/adapter.rs. */
const BASE_CAPS: AdapterCap[] = ["STRUCTURED_OUTPUT", "RESUME", "MODEL_SELECT", "READ_ONLY_MODE"];
const MOCK_ADAPTERS: Partial<Record<ProviderId, AdapterInfo>> = {
  CLAUDE_CODE: { caps: [...BASE_CAPS, "NATIVE_FORK", "TOOL_RULES"], versions: ">= 2.1.288, < 3", tested: "2.1.288" },
  CODEX_CLI: { caps: [...BASE_CAPS, "OWN_SANDBOX", "NATIVE_WORKSPACE_SANDBOX"], versions: ">= 0.160.0, < 1", tested: "0.160.0" },
  CURSOR_CLI: { caps: [...BASE_CAPS, "DENIAL_EVENT"], versions: ">= 2026.10.01", tested: "2026.10.01" },
  COPILOT_CLI: { caps: [...BASE_CAPS, "TOOL_RULES"], versions: ">= 1.0.91, < 2", tested: "1.0.91" },
  ANTIGRAVITY_CLI: { caps: [...BASE_CAPS, "TOOL_RULES", "DENIAL_EVENT", "OS_SANDBOX", "SHELL_WRAP"], versions: ">= 1.0.0, < 3", tested: "1.14.0" },
  AIDER: { caps: ["MODEL_SELECT", "READ_ONLY_MODE", "RESUME", "OS_SANDBOX"], versions: ">= 0.50.0, < 2", tested: "0.72.0" },
  OPENCODE: { caps: [...BASE_CAPS, "TOOL_RULES", "DENIAL_EVENT", "OS_SANDBOX", "SHELL_WRAP"], versions: ">= 0.1.0, < 2", tested: "0.8.4" },
  REASONIX: {
    caps: [...BASE_CAPS, "NATIVE_FORK", "TOOL_RULES", "DENIAL_EVENT", "OS_SANDBOX", "SHELL_WRAP", "NATIVE_SESSIONS"],
    versions: ">= 1.0.0",
    tested: "2.31.0",
  },
};

/** Mirrors `trust_args` in src-tauri/src/providers.rs. */
const MOCK_TRUST: Partial<Record<ProviderId, string[]>> = {
  CLAUDE_CODE: ["--add-dir", "<workspace>"],
  CODEX_CLI: ["--skip-git-repo-check"],
  CURSOR_CLI: ["--trust", "--workspace", "<workspace>"],
  COPILOT_CLI: ["--add-dir", "<workspace>"],
  ANTIGRAVITY_CLI: ["--workspace", "<workspace>"],
  OPENCODE: ["--workspace", "<workspace>"],
};

const single = (m: ModelInfo, efforts: string[] = [], fast = false): ModelGroup => ({
  key: m.id,
  name: m.name,
  efforts,
  defaultEffort: efforts[1] ?? null,
  contexts: [],
  thinking: false,
  fast,
  tags: [],
  tiers: [],
  defaultTier: null,
  variants: [{ id: m.id, effort: null, context: null, thinking: false, fast: false }],
});

const MOCK_RAW: MockProvider[] = [
  {
    id: "CLAUDE_CODE",
    name: "Claude Code",
    executable: "claude",
    featured: true,
    status: "READY",
    version: "2.1.0",
    path: "/opt/homebrew/bin/claude",
    installMethods: [
      { kind: "SCRIPT", command: "curl -fsSL https://claude.ai/install.sh | bash", recommended: true, available: true, requires: null },
      { kind: "HOMEBREW", command: "brew install --cask claude-code", recommended: false, available: true, requires: null },
      { kind: "NPM", command: "npm install -g @anthropic-ai/claude-code", recommended: false, available: true, requires: "Node.js 22+" },
    ],
    authMethod: "oauth_token",
    account: { billing: "PAY_AS_YOU_GO", official: false, endpoint: "https://relay.example.org/v1/***", plan: null, balance: null, usage: null },
    models: ["fable", "opus", "opus[1m]", "sonnet", "sonnet[1m]", "haiku"].map((id) => ({
      id,
      name: id[0].toUpperCase() + id.slice(1).replace("[1m]", " 1M"),
    })),
    modelGroups: [
      single({ id: "fable", name: "Fable" }),
      ...["opus", "sonnet"].map((k) => ({
        ...single({ id: k, name: k[0].toUpperCase() + k.slice(1) }),
        contexts: ["default", "1m"],
        variants: [
          { id: k, effort: null, context: null, thinking: false, fast: false },
          { id: `${k}[1m]`, effort: null, context: "1m", thinking: false, fast: false },
        ],
      })),
      single({ id: "haiku", name: "Haiku" }),
    ],
    modelsSource: "BUILTIN",
    guidance: [],
  },
  {
    id: "CODEX_CLI",
    name: "Codex CLI",
    executable: "codex",
    featured: true,
    status: "READY",
    version: "0.160.0",
    path: "/opt/homebrew/bin/codex",
    installMethods: [
      { kind: "SCRIPT", command: "curl -fsSL https://chatgpt.com/codex/install.sh | sh", recommended: true, available: true, requires: null },
      { kind: "HOMEBREW", command: "brew install --cask codex", recommended: false, available: true, requires: null },
      { kind: "NPM", command: "npm install -g @openai/codex", recommended: false, available: true, requires: null },
    ],
    authMethod: "ChatGPT",
    account: {
      billing: "SUBSCRIPTION",
      official: true,
      endpoint: null,
      plan: "plus",
      balance: null,
      usage: {
        windows: [
          { windowMinutes: 300, usedPercent: 24, resetsAt: Math.floor(Date.now() / 1000) + 2 * 3600 },
          { windowMinutes: 10080, usedPercent: 86, resetsAt: Math.floor(Date.now() / 1000) + 3 * 86400 },
        ],
        observedAt: new Date(Date.now() - 6 * 60_000).toISOString(),
        source: "SESSION_LOG",
      },
    },
    models: [
      { id: "gpt-6.1-sol", name: "GPT-6.1-Sol" },
      { id: "gpt-6-luna", name: "GPT-6-Luna" },
    ],
    modelGroups: [
      single({ id: "gpt-6.1-sol", name: "GPT-6.1-Sol" }, ["low", "medium", "high", "xhigh", "max", "ultra"], true),
      single({ id: "gpt-6-luna", name: "GPT-6-Luna" }, ["low", "medium", "high", "xhigh", "max"], true),
    ],
    modelsSource: "CLI",
    guidance: [],
  },
  {
    id: "ANTIGRAVITY_CLI",
    name: "Antigravity CLI",
    executable: "agy",
    featured: true,
    status: "READY",
    version: "1.2.16",
    path: "/Users/me/.local/bin/agy",
    authMethod: "Google account",
    account: { billing: "SUBSCRIPTION", official: true, endpoint: null, plan: null, balance: null, usage: null },
    models: [
      { id: "gemini-3.8-flash-high", name: "Gemini 3.8 Flash (High)" },
      { id: "gemini-3.8-flash-low", name: "Gemini 3.8 Flash (Low)" },
      { id: "gemini-3.1-pro-high", name: "Gemini 3.1 Pro (High)" },
    ],
    modelGroups: [
      {
        key: "gemini-3.8-flash",
        name: "Gemini 3.8 Flash",
        efforts: ["low", "high"],
        defaultEffort: null,
        contexts: [],
        thinking: false,
        fast: false,
        tags: [],
        tiers: [],
        defaultTier: null,
        variants: [
          { id: "gemini-3.8-flash-low", effort: "low", context: null, thinking: false, fast: false },
          { id: "gemini-3.8-flash-high", effort: "high", context: null, thinking: false, fast: false },
        ],
      },
      single({ id: "gemini-3.1-pro-high", name: "Gemini 3.1 Pro (High)" }),
    ],
    modelsSource: "CLI",
    guidance: [],
    installMethods: [
      { kind: "SCRIPT", command: "curl -fsSL https://antigravity.google/cli/install.sh | bash", recommended: true, available: true, requires: null },
    ],
  },
  {
    id: "AIDER",
    name: "Aider",
    executable: "aider",
    featured: false,
    status: "NOT_INSTALLED",
    version: null,
    path: null,
    installMethods: [
      { kind: "SCRIPT", command: "curl -LsSf https://aider.chat/install.sh | sh", recommended: true, available: true, requires: null },
      { kind: "HOMEBREW", command: "brew install aider", recommended: false, available: true, requires: null },
      { kind: "UV", command: "uv tool install --force --python python3.12 --with pip aider-chat@latest", recommended: false, available: false, requires: null },
      { kind: "PIPX", command: "pipx install aider-chat", recommended: false, available: false, requires: null },
    ],
    authMethod: null,
    models: [],
    modelsSource: null,
    guidance: [{ code: "INSTALL", command: null }],
  },
];

// Browser preview only: Cursor with Auto, own-pool (Grok) and third-party models.
const cursorVariants = (base: string, efforts: string[], fastFor: string[]) =>
  efforts.flatMap((e) =>
    [false, true]
      .filter((f) => !f || fastFor.includes(e))
      .map((fast) => ({ id: `${base}-${e}${fast ? "-fast" : ""}`, effort: e, context: null, thinking: false, fast })),
  );
MOCK_RAW.push({
  id: "CURSOR_CLI",
  name: "Cursor CLI",
  executable: "cursor-agent",
  featured: true,
  status: "READY",
  version: "2026.09.30",
  path: "/Users/me/.local/bin/cursor-agent",
  authMethod: "Cursor account",
  installMethods: [],
  models: [],
  modelGroups: [
    single({ id: "auto", name: "Auto" }),
    {
      key: "cursor-grok-4.6",
      name: "Grok 4.6",
      efforts: ["low", "medium", "high", "xhigh"],
      defaultEffort: null,
      contexts: [],
      thinking: false,
      fast: true,
      tags: [],
      tiers: [],
      defaultTier: null,
      variants: cursorVariants("cursor-grok-4.6", ["low", "medium", "high", "xhigh"], ["low", "medium", "high", "xhigh"]),
    },
    {
      key: "claude-opus-5-5",
      name: "Claude Opus 5.5",
      efforts: ["low", "medium", "high", "xhigh", "max"],
      defaultEffort: null,
      contexts: [],
      thinking: false,
      fast: true,
      tags: ["1m"],
      tiers: [],
      defaultTier: null,
      variants: cursorVariants("claude-opus-5-5", ["low", "medium", "high", "xhigh", "max"], ["low", "max"]),
    },
    {
      key: "claude-fable-5",
      name: "Claude Fable 5",
      efforts: ["low", "high"],
      defaultEffort: null,
      contexts: [],
      thinking: false,
      fast: false,
      tags: ["no-zdr", "1m"],
      tiers: [],
      defaultTier: null,
      variants: cursorVariants("claude-fable-5", ["low", "high"], []),
    },
    single({ id: "gpt-5.2", name: "GPT-5.2" }),
  ],
  modelsSource: "CLI",
  guidance: [],
});

// Browser preview only: a few not-installed whitelist entries so the "Add CLI" picker has content.
const notInstalled = (id: ProviderId, name: string, executable: string, featured = false): MockProvider => ({
  id,
  name,
  executable,
  featured,
  status: "NOT_INSTALLED",
  version: null,
  path: null,
  authMethod: null,
  models: [],
  modelsSource: null,
  guidance: [{ code: "INSTALL", command: null }],
  installMethods: [],
});
MOCK_RAW.push(
  notInstalled("COPILOT_CLI", "GitHub Copilot CLI", "copilot"),
  notInstalled("OPENCODE", "OpenCode", "opencode", true),
  notInstalled("REASONIX", "Reasonix", "reasonix"),
  notInstalled("GROK_CLI", "Grok Build", "grok"),
  notInstalled("KIMI_CODE", "Kimi Code", "kimi"),
);

const MOCK_PROVIDERS: ProviderInfo[] = MOCK_RAW.map((p) => ({
  ...p,
  account: p.account ?? null,
  modelGroups: p.modelGroups ?? p.models.map((m) => single(m)),
  trustArgs: p.trustArgs ?? MOCK_TRUST[p.id] ?? [],
  chat: p.id in MOCK_ADAPTERS,
  adapter: MOCK_ADAPTERS[p.id] ?? null,
  directLimit: p.id === "CODEX_CLI" ? { code: "CODEX_READ_ONLY" } : null,
}));

export async function getDebugProviderCatalog(): Promise<ProviderInfo[]> {
  if (!inTauri) return (await import("./debugProvider")).builtinDebugCatalog();
  return invoke<ProviderInfo[]>("get_debug_provider_catalog");
}

export interface ProviderDetected {
  scanId: string;
  provider: ProviderInfo;
}

let mockDetected: ((e: ProviderDetected) => void) | null = null;

/** Fires once per CLI as soon as its probe finishes during `refreshProviders`. */
export async function onProviderDetected(cb: (e: ProviderDetected) => void): Promise<() => void> {
  if (!inTauri) {
    mockDetected = cb;
    return () => {
      mockDetected = null;
    };
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ProviderDetected>("provider-detected", (e) => cb(e.payload));
}

/** Rescans every whitelisted CLI. Results stream via `onProviderDetected`; the full list resolves at the end. */
export async function refreshProviders(
  scanId: string,
  customPaths: Partial<Record<ProviderId, string>>,
): Promise<ProviderInfo[]> {
  if (!inTauri) {
    // Browser preview: stagger results to mimic probes finishing at different times.
    for (const [i, provider] of MOCK_PROVIDERS.entries()) {
      setTimeout(() => mockDetected?.({ scanId, provider }), 300 + i * 250);
    }
    await new Promise((r) => setTimeout(r, 300 + MOCK_PROVIDERS.length * 250));
    return MOCK_PROVIDERS;
  }
  return invoke<ProviderInfo[]>("refresh_providers", { scanId, customPaths });
}

/** Re-detects one CLI only; the backend patches its cached list too. */
export async function refreshProvider(id: ProviderId, customPath?: string): Promise<ProviderInfo> {
  if (!inTauri) {
    await new Promise((r) => setTimeout(r, 600));
    const found = MOCK_PROVIDERS.find((p) => p.id === id);
    if (!found) throw new Error(`Unknown provider: ${id}`);
    return found;
  }
  return invoke<ProviderInfo>("refresh_provider", { id, customPath: customPath?.trim() || null });
}

/** Explicit BYOK authentication check; sends one small Gemini prompt. */
export async function verifyAntigravityApiKey(): Promise<ProviderInfo> {
  if (!inTauri) throw new Error("API key verification requires the desktop app");
  return invoke<ProviderInfo>("verify_antigravity_api_key");
}

export async function verifyReasonixApiKey(model?: string): Promise<ProviderInfo> {
  if (!inTauri) throw new Error("API key verification requires the desktop app");
  return invoke<ProviderInfo>("verify_reasonix_api_key", { model: model ?? null });
}

export async function reloadShellEnv(): Promise<void> {
  if (!inTauri) return;
  await invoke("reload_shell_env");
}

export interface GitInfo {
  isRepo: boolean;
  branch: string | null;
  detached: boolean;
  /** False in a repository with no commit yet. */
  hasCommits: boolean;
  upstream: string | null;
  ahead: number;
  behind: number;
  /** Entries in `git status` (staged, unstaged, untracked, conflicted). */
  changes: number;
  staged: number;
  unstaged: number;
  untracked: number;
  conflicted: number;
  /** Line counts of tracked changes against HEAD. */
  insertions: number;
  deletions: number;
  /** A linked worktree rather than the repository's main checkout. */
  linkedWorktree: boolean;
  root: string | null;
  headAuthorName?: string | null;
  headAuthorEmail?: string | null;
  headCoAuthors?: string[];
  headCoAuthoredByTokage?: boolean;
}

export async function gitInfo(path: string): Promise<GitInfo> {
  if (!inTauri) {
    const wt = path.includes("/worktrees/");
    return {
      isRepo: true,
      branch: wt ? "tokage/" + path.split("/").pop() : "main",
      detached: false,
      hasCommits: true,
      upstream: wt ? null : "origin/main",
      ahead: wt ? 0 : 2,
      behind: 0,
      changes: 3,
      staged: 1,
      unstaged: 1,
      untracked: 1,
      conflicted: 0,
      insertions: 24,
      deletions: 7,
      linkedWorktree: wt,
      root: path,
    };
  }
  return invoke<GitInfo>("git_info", { path });
}

export interface GitBranchInfo {
  name: string;
  current: boolean;
}

export interface GitStatusFile {
  path: string;
  /** added | modified | deleted | untracked | conflicted | renamed */
  change: string;
  insertions: number;
  deletions: number;
}

export interface GitFileDiff {
  path: string;
  /** Maps to preview kinds: added | modified | deleted */
  change: string;
  diff: string | null;
  insertions: number;
  deletions: number;
}

export async function gitListBranches(path: string): Promise<GitBranchInfo[]> {
  if (!inTauri) {
    return [
      { name: "main", current: true },
      { name: "feature/demo", current: false },
    ];
  }
  return invoke<GitBranchInfo[]>("git_list_branches", { path });
}

/** Throws `"DIRTY"` when the working tree has uncommitted changes. */
export async function gitCheckoutBranch(path: string, branch: string): Promise<void> {
  if (!inTauri) return;
  await invoke("git_checkout_branch", { path, branch });
}

/** Throws `"DIRTY"` when the working tree has uncommitted changes. */
export async function gitCreateBranch(path: string, branch: string): Promise<void> {
  if (!inTauri) return;
  await invoke("git_create_branch", { path, branch });
}

export async function gitStatusFiles(path: string): Promise<GitStatusFile[]> {
  if (!inTauri) {
    return [
      { path: "src/App.tsx", change: "modified", insertions: 12, deletions: 3 },
      { path: "README.md", change: "modified", insertions: 8, deletions: 2 },
      { path: "new-file.ts", change: "untracked", insertions: 4, deletions: 0 },
    ];
  }
  return invoke<GitStatusFile[]>("git_status_files", { path });
}

export async function gitFileDiff(path: string, file: string): Promise<GitFileDiff> {
  if (!inTauri) {
    return {
      path: file,
      change: "modified",
      diff: `--- a/${file}\n+++ b/${file}\n@@ -1,3 +1,4 @@\n line1\n-line2\n+line2 changed\n line3\n+line4\n`,
      insertions: 2,
      deletions: 1,
    };
  }
  return invoke<GitFileDiff>("git_file_diff", { path, file });
}

export interface Worktree {
  /** Worktree checkout root. */
  root: string;
  /** Where turns run: the project's own folder inside the worktree. */
  cwd: string;
  branch: string;
}

/** New linked worktree on branch `tokage/<name>` from `baseRef` (default HEAD). */
export async function createSessionWorktree(
  path: string,
  name: string,
  baseRef?: string | null,
): Promise<Worktree> {
  if (!inTauri) {
    const root = `~/Library/Application Support/dev.tokage.app/worktrees/mock/${name}`;
    return { root, cwd: root, branch: `tokage/${name}` };
  }
  return invoke<Worktree>("create_session_worktree", {
    path,
    name,
    baseRef: baseRef ?? null,
  });
}

/**
 * Removes a session worktree. Rejects with "DIRTY" when the checkout has uncommitted
 * changes and `force` is false. `keepBranch` retains the `tokage/` branch (F11).
 */
export async function removeSessionWorktree(
  root: string,
  branch: string,
  force: boolean,
  keepBranch = false,
): Promise<void> {
  if (!inTauri) return;
  await invoke("remove_session_worktree", { root, branch, force, keepBranch });
}

/** F11: stage + commit in a task worktree. Returns false if nothing to commit. */
export async function commitTaskWorktree(
  root: string,
  message: string,
  expectedScope: ScopeSnapshot,
  coAuthor = false,
): Promise<boolean> {
  if (!inTauri) return false;
  return invoke<boolean>("commit_task_worktree", { root, message, expectedScope, coAuthor });
}

export type WorktreeMergeOutcome =
  | { success: { mergedBranch: string } }
  | {
      conflict: {
        diagnostic?: string;
        conflictedFiles: string[];
        integrationTask: TaskItem;
      };
    };

/** F11: merge task branch into integration worktree (rejects main checkout paths). */
export async function mergeTaskWorktree(
  integrationRoot: string,
  integrationBranch: string,
  taskBranch: string,
  task: TaskItem,
  expectedCommit: string,
  coAuthor = false,
): Promise<WorktreeMergeOutcome> {
  if (!inTauri) {
    return { success: { mergedBranch: integrationBranch } };
  }
  return invoke<WorktreeMergeOutcome>("merge_task_worktree_cmd", {
    integrationRoot,
    integrationBranch,
    taskBranch,
    task,
    expectedCommit,
    coAuthor,
  });
}

/** F11.e: create worktree from integration + pre-merge task branch (conflict markers ok). */
export async function prepareIntegrationWorktree(
  projectPath: string,
  name: string,
  integrationBranch: string,
  taskBranch: string,
  existingWorktree: Worktree,
  expectedCommit: string,
  expectedConflicts: string[],
): Promise<Worktree> {
  if (!inTauri) {
    const root = `~/Library/Application Support/dev.tokage.app/worktrees/mock/${name}`;
    return { root, cwd: root, branch: `tokage/${name}` };
  }
  return invoke<Worktree>("prepare_integration_worktree", {
    projectPath,
    name,
    integrationBranch,
    taskBranch,
    existingWorktree,
    expectedCommit,
    expectedConflicts,
  });
}

/** F11: remove task worktree; keepBranch defaults true. */
export async function removeTaskWorktree(
  root: string,
  branch: string,
  keepBranch = true,
  force = false,
  expectedCommit?: string,
): Promise<void> {
  if (!inTauri) return;
  await invoke("remove_task_worktree", { root, branch, keepBranch, force, expectedCommit: expectedCommit ?? null });
}

export async function gitRevParse(path: string, rev?: string | null): Promise<string> {
  if (!inTauri) return "mock-sha";
  return invoke<string>("git_rev_parse", { path, rev: rev ?? null });
}

export interface CheckoutIdentity { head: string; reference: string }
export async function gitCheckoutIdentity(path: string): Promise<CheckoutIdentity> {
  if (!inTauri) return { head: await gitRevParse(path, "HEAD"), reference: "refs/heads/main" };
  return invoke<CheckoutIdentity>("git_checkout_identity", { path });
}

export interface ScopeSnapshot {
  /** Set only on the persisted execution baseline, never inferred again from mutable settings. */
  executionMode?: "worktree" | "inplace";
  root: string;
  workdir: string;
  prefix: string;
  clean: boolean;
  head: string;
  files: Record<string, { disk: string; index: string }>;
}

export async function gitScopeSnapshot(workdir: string): Promise<ScopeSnapshot> {
  if (!inTauri) return { root: workdir, workdir, prefix: "", clean: true, head: "mock-sha", files: {} };
  return invoke<ScopeSnapshot>("git_scope_snapshot", { workdir });
}

/** Content backup must complete before launching any pipeline CLI. */
export async function gitCaptureRollback(workdir: string, turnId: string, expected: ScopeSnapshot): Promise<void> {
  if (!inTauri) return;
  await invoke("git_capture_rollback", { workdir, turnId, expected });
}
export async function gitRollbackChanges(workdir: string, turnId: string): Promise<string[]> {
  if (!inTauri) throw new Error("Rollback requires native content backups");
  return invoke<string[]>("git_rollback_changes", { workdir, turnId });
}

/** F11.f: merge integration branch into user main checkout. Throws `"DIRTY"` / `"CONFLICT"`. */
export async function mergeIntegrationToCheckout(
  projectPath: string,
  branch: string,
  expected: ScopeSnapshot,
  target?: CheckoutIdentity | null,
  coAuthor = false,
): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("merge_integration_to_checkout", {
    projectPath,
    branch,
    expected,
    target: target ?? null,
    coAuthor,
  });
}

/** F11.f: write `git diff fromRef..toRef` to outPath. */
export async function gitContainsCommit(path: string, branch: string, source: string): Promise<boolean> {
  if (!inTauri) return true;
  return invoke("git_contains_commit", { path, branch, source });
}

export async function reconcileIntegrationDelivery(path: string, source: string, target: { head: string; reference: string }): Promise<boolean> {
  if (!inTauri) return false;
  return invoke("reconcile_integration_delivery", { path, source, target });
}

export async function exportIntegrationPatch(
  projectPath: string,
  fromRef: string,
  toRef: string,
  outPath: string,
  allowIdentical = false,
): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("export_integration_patch", {
    projectPath,
    fromRef,
    toRef,
    outPath,
    allowIdentical,
  });
}

/** F11.g: `git worktree prune`. */
export async function pruneProjectWorktrees(projectPath: string): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("prune_project_worktrees", { projectPath });
}

export interface OrphanWorktreeEntry {
  path: string;
  branch?: string | null;
  dirty: boolean;
}

/** F11.g: scan `{app_data}/worktrees/` for leftover checkouts. */
export async function listOrphanWorktrees(): Promise<OrphanWorktreeEntry[]> {
  if (!inTauri) return [];
  return invoke<OrphanWorktreeEntry[]>("list_orphan_worktrees");
}

/** F11.g: prune stale worktree registrations at app start. */
export async function reconcileWorktreesOnStartup(): Promise<number> {
  if (!inTauri) return 0;
  return invoke<number>("reconcile_worktrees_on_startup");
}

/** F11.g: force-remove an orphan under app worktrees/. */
export async function removeOrphanWorktree(
  root: string,
  branch?: string | null,
): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("remove_orphan_worktree", {
    root,
    branch: branch ?? null,
  });
}

export interface FileText {
  /** Null for binary or oversized files. */
  text: string | null;
  binary: boolean;
  size: number;
}

/** Reads a text file inside `root` for the preview pane. */
export async function readProjectFile(root: string, path: string): Promise<FileText> {
  if (!inTauri) {
    const text = MOCK_FILES[path] ?? MOCK_FILES[`${root}/${path}`] ?? MOCK_FILES[path.split("/").pop() ?? ""] ?? null;
    if (text === null) throw new Error(`File not found: ${path}`);
    return { text, binary: false, size: text.length };
  }
  return invoke<FileText>("read_project_file", { root, path });
}

/** Build provenance, fixed at compile time (docs/11 §7.1). */
export interface BuildInfo {
  version: string;
  commit: string | null;
  kind: "official" | "local";
  repoUrl: string;
  /** Tree URL for this exact source: the release tag, else the commit. */
  sourceUrl: string;
  releaseSha256?: string;
  macosTeamId?: string;
  windowsSigner?: string;
  /** Official knowledge-pack update channel enabled (F8 / D6). */
  knowledgeUpdateEnabled: boolean;
}

export const REPO_URL = "https://github.com/iwpz/tokage";

export async function getBuildInfo(): Promise<BuildInfo> {
  if (!inTauri) {
    return {
      version: __APP_VERSION__,
      commit: null,
      kind: "local",
      repoUrl: REPO_URL,
      sourceUrl: `${REPO_URL}/tree/master`,
      releaseSha256: "https://github.com/iwpz/tokage/releases/latest/download/SHA256SUMS",
      macosTeamId: "Developer ID Application: Tokage OpenSource Project (TEAM_ID_TOKAGE_PROD)",
      windowsSigner: "CN=Tokage OpenSource Project, O=Tokage Team, L=Tokyo, C=JP",
      // Browser/dev stub mirrors Rust default (no official pubkey).
      knowledgeUpdateEnabled: false,
    };
  }
  return invoke<BuildInfo>("build_info");
}

/** Opens an http(s) link in the default browser. */
export async function openExternalUrl(url: string): Promise<void> {
  if (!inTauri) {
    window.open(url, "_blank", "noopener");
    return;
  }
  await invoke("open_external_url", { url });
}

/** Downloads a knowledge pack from a custom URL (https, or http on localhost) as raw text. */
export async function fetchKnowledgePack(url: string): Promise<string> {
  if (!inTauri) {
    const res = await fetch(url.trim());
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return res.text();
  }
  return invoke<string>("fetch_knowledge_pack", { url });
}

export interface StagedUpdateInfo {
  packVersion: string;
  pendingPath: string;
  sha256: string;
}

/** Checks the official release host for updated knowledge pack, verifies minisign & stages to pending.json (P8.m4a). */
export async function checkKnowledgePackUpdate(
  currentPackVersion: string,
  metadataUrl?: string
): Promise<StagedUpdateInfo | null> {
  if (!inTauri) return null;
  return invoke<StagedUpdateInfo | null>("check_knowledge_pack_update", {
    currentPackVersion,
    metadataUrl,
  });
}

/** Returns info on staged pending knowledge pack if pending.json exists (P8.m4a). */
export async function getPendingKnowledgeUpdate(): Promise<StagedUpdateInfo | null> {
  if (!inTauri) return null;
  return invoke<StagedUpdateInfo | null>("get_pending_knowledge_update");
}

/** Atomically applies the staged pending knowledge pack into SQLite (P8.m4a). */
export async function applyKnowledgePackUpdate(): Promise<any> {
  if (!inTauri) return null;
  return invoke<any>("apply_knowledge_pack_update");
}

export type OpenTarget = "FILE_MANAGER" | "VS_CODE" | "TERMINAL";

export async function listOpenTargets(): Promise<OpenTarget[]> {
  if (!inTauri) return ["FILE_MANAGER", "VS_CODE", "TERMINAL"];
  return invoke<OpenTarget[]>("list_open_targets");
}

export async function openPathIn(path: string, target: OpenTarget): Promise<void> {
  if (!inTauri) {
    console.info(`[mock] open ${path} in ${target}`);
    return;
  }
  await invoke("open_path_in", { path, target });
}

export async function pickProjectDirectory(): Promise<string | null> {
  if (!inTauri) {
    const p = window.prompt("Project path", "~/code/my-project");
    return p?.trim() || null;
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({ directory: true, multiple: false });
  return typeof result === "string" ? result : null;
}

// Project context directory `.tokage/` (src-tauri/crates/tokage-native/src/context.rs).
export interface SavedPlan {
  path: string;
  version: number;
}

export async function saveSessionPlan(cwd: string, sessionId: string, title: string, text: string): Promise<SavedPlan | null> {
  if (!inTauri) return null;
  return invoke<SavedPlan>("save_session_plan", { cwd, sessionId, title, text });
}

export async function tokageDirExists(cwd: string): Promise<boolean> {
  return inTauri ? invoke<boolean>("tokage_dir_exists", { cwd }) : true;
}

/** Whether `path` is an existing directory (always true outside Tauri). */
export async function pathIsDir(path: string): Promise<boolean> {
  return inTauri ? invoke<boolean>("path_is_dir", { path }) : true;
}

export async function ignoreTokageDir(path: string): Promise<void> {
  if (inTauri) await invoke("ignore_tokage_dir", { path });
}

export async function removeTokageDir(path: string): Promise<void> {
  if (inTauri) await invoke("remove_tokage_dir", { path });
}

/** Yes / no question in a native dialog. */
export async function askYesNo(message: string, title: string): Promise<boolean> {
  if (!inTauri) return window.confirm(message);
  const { ask } = await import("@tauri-apps/plugin-dialog");
  return ask(message, { title, kind: "info" });
}

// Conversation turns (src-tauri/src/chat.rs). Output streams back as `chat-event` events.
export type TurnMode = "plan" | "direct";
export type AgentRole = "PLANNER" | "EXECUTOR" | "TESTER" | "REVIEWER";

export interface ChatRequest {
  turnId: string;
  cwd: string;
  prompt: string;
  mode: TurnMode;
  provider: ProviderId;
  model: string;
  /** Reasoning effort / speed tier for CLIs that take them as parameters (Codex). */
  effort: string | null;
  fast: boolean;
  /** Routing profile for a CLI's auto model (Copilot `--auto-tier`). */
  tier: string | null;
  history: { role: "user" | "assistant"; text: string }[];
  resume: string | null;
  /** Continue `resume` as a new CLI session (Claude Code `--fork-session`). */
  fork?: boolean;
  /** CLIs the user allowed to receive workspace trust; the backend runs no others. */
  trusted: ProviderId[];
  /** The session's saved plan under `.tokage/plans/`, relative to `cwd`. */
  planFile?: string | null;
  sessionId?: string | null;
  projectId?: string | null;
  sessionAllowed?: string[];
  authMode?: AuthMode | null;
  /** Task role of the agent/window (docs/01 §6, docs/11 §2). */
  role?: AgentRole | null;
  /** Allowed write paths for EXECUTOR / TESTER (docs/01 §3, docs/09 §4, docs/11 §2). */
  writeScope?: string[];
  /** Timeout tier (docs/09 §2.1, R2). Defaults based on mode if omitted. `AGENT` for sub-agents (F3). */
  timeoutTier?: "QUICK" | "STANDARD" | "LONG" | "PLANNING" | "GATE" | "AGENT" | null;
  /** Timeout multiplier from settings (e.g. 1.0, 2.0). Ignored for `AGENT`. */
  timeoutMultiplier?: number | null;
  /** Agent-tier idle timeout in seconds (default 1800). */
  idleTimeoutSecs?: number | null;
  /** Agent-tier hard timeout in seconds (default 7200). */
  hardTimeoutSecs?: number | null;
  /** `plans.id` for usage attribution (F12). */
  planId?: number | null;
  /** `tasks.id` (integer PK) for usage attribution (F12). */
  taskId?: number | null;
  taskCode?: string | null;
  /** Task domain for usage attribution (F12). */
  domain?: string | null;
  /** Usage phase override (F12). Pipeline dispatch uses EXECUTION; repair uses REPAIR. */
  phase?: "PLANNING" | "EXECUTION" | "REPAIR" | "CONTEXT" | "DIRECT" | null;
  /** `usage_events.attempt_id` (F12). Pipeline passes turnId. */
  attemptId?: string | null;
  debug?: DebugProviderConfig;
}

export type ToolKind = "command" | "read" | "search" | "edit" | "web" | "agent" | "other";
export type FileChangeKind = "added" | "modified" | "deleted";

export type ChatEventKind =
  | { kind: "structuredOutput"; json: string }
  | ({ kind: "debugRoute" } & DebugRoute)

  | { kind: "thread"; id: string }
  /** The model the CLI reports running (a resolved alias, or an auto router's pick). */
  | { kind: "model"; name: string }
  | { kind: "reasoning"; text: string }
  | { kind: "text"; text: string }
  | { kind: "toolStart"; id: string; tool: ToolKind; title: string; detail: string | null }
  | { kind: "toolEnd"; id: string; ok: boolean | null; output: string | null }
  | {
      kind: "fileChange";
      toolId: string | null;
      /** Relative to the turn's working directory when inside it. */
      path: string;
      absPath: string;
      /** Tokage's copy of a file written outside the working directory, readable by the preview. */
      previewPath: string | null;
      change: FileChangeKind;
      /** Unified diff; null when the previous content is unknown or the file is too large. */
      diff: string | null;
      additions: number;
      deletions: number;
    }
  | {
      kind: "approvalRequest";
      id: string;
      toolId: string | null;
      command: string;
      level: string;
      reason: string;
      recoverable: boolean | null;
    }
  | {
      kind: "approvalResolved";
      id: string;
      decision: string;
    }
  | {
      kind: "done";
      inputTokens: number;
      outputTokens: number;
      cacheReadTokens?: number;
      cacheWriteTokens?: number;
      apiCalls?: number | null;
      contextTokens?: number | null;
      costUsd: number | null;
      premiumRequests: number | null;
      credits?: number | null;
      source?: "STRUCTURED" | "STDOUT_REGEX" | "ESTIMATED";
    }
  | { kind: "error"; message: string }
  | { kind: "warning"; message: string }
  | { kind: "quota"; raw: string; resetAt?: string | null; retrying: boolean };

export type ChatEvent = { turnId: string } & ChatEventKind;

const mockTimers = new Map<string, number>();
const mockListeners = new Set<(e: ChatEvent) => void>();

export async function onChatEvent(cb: (e: ChatEvent) => void): Promise<() => void> {
  if (!inTauri) {
    mockListeners.add(cb);
    return () => {
      mockListeners.delete(cb);
    };
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ChatEvent>("chat-event", (e) => {
    if (e.payload.kind === "debugRoute") rememberDebugRoute(e.payload.turnId, e.payload);
    cb(e.payload);
  });
}

/** Fired once when a turn ends (normal, failed, or cancelled). */
export type ChatEndedEvent = { turnId: string; cancelled: boolean };

const mockEndedListeners = new Set<(e: ChatEndedEvent) => void>();

export async function onChatEnded(cb: (e: ChatEndedEvent) => void): Promise<() => void> {
  if (!inTauri) {
    mockEndedListeners.add(cb);
    return () => {
      mockEndedListeners.delete(cb);
    };
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ChatEndedEvent>("chat-ended", (e) => cb(e.payload));
}

/** Test/browser helper: emit a synthetic `chat-ended` to mock listeners. */
export function emitMockChatEnded(event: ChatEndedEvent): void {
  mockEndedListeners.forEach((fn) => fn(event));
}

export async function startChatTurn(request: ChatRequest): Promise<void> {
  const config = demoConfig(request.sessionId, useSettings.getState().demoChunkDelayMs) ?? request.debug ?? debugConfig();
  registerDemoTurn(request);
  rememberDebugTurn(request, config);
  request = { ...request, debug: config };
  if (!inTauri) {
    if (config.output === "luna" || config.output === "deepseek") throw new Error(i18n.t("debugProvider.browserRealUnavailable"));
    const steps = mockTurn(request);
    let i = 0;
    const readyAt = Date.now() + (config.scenario === "slow" && config.output === "mock" && (!config.demo || request.taskCode === "T3") ? 30_000 : 0);
    const emit = (k: ChatEventKind) => {
      const payload: ChatEvent = { turnId: request.turnId, ...k };
      if (payload.kind === "debugRoute") rememberDebugRoute(request.turnId, payload);
      mockListeners.forEach((fn) => fn(payload));
    };
    const timer = globalThis.setInterval(() => {
      if (Date.now() < readyAt) return;
      if (i < steps.length) emit(steps[i++]);
      else {
        globalThis.clearInterval(timer);
        mockTimers.delete(request.turnId);
        emitMockChatEnded({ turnId: request.turnId, cancelled: false });
      }
    }, config.output === "mock" ? Math.max(1, config.chunkDelayMs) : 60);
    mockTimers.set(request.turnId, timer as any);
    return;
  }
  if (config.output === "mock") request.cwd = await prepareDebugWorkspace(request.cwd);
  if (config.output === "luna" || config.output === "deepseek") {
    // Vite can reload the frontend while an older desktop backend is still running.
    // Prove debug routing exists before sending a logical (possibly expensive) model.
    try { await getDebugProviderCatalog(); }
    catch { throw new Error(i18n.t("debugProvider.backendUnavailable")); }
  }
  await invoke("start_chat_turn", { request });
}

export function seedDemoBrowserFiles(cwd: string): void {
  if (inTauri) return;
  for (const [path, content] of Object.entries(demoFixture.initialFiles)) MOCK_FILES[`${cwd}/${path}`] = content;
}

const browserDemoPlans = new Map<string, Plan>();
const MOCK_FILES: Record<string, string> = {
  "m.py": "def add(a, b):\n    return a + b\n",
  "test_m.py": "from m import add\n\n\ndef test_add():\n    assert add(2, 3) == 5\n",
};

function mockTurn(r: ChatRequest): ChatEventKind[] {
  if (r.debug?.output === "mock") {
    const route: ChatEventKind = { kind: "debugRoute", output: "mock", requestedProvider: r.provider, requestedModel: r.model, actualProvider: "DEBUG_PROVIDER", actualModel: "mock", workdir: r.cwd };
    const output: ChatEventKind[] = [route, { kind: "thread", id: `tokage-debug:${JSON.stringify(["mock", r.provider, r.model, r.turnId])}` }, { kind: "model", name: "mock" }];
    if (r.debug.scenario === "error" && (!r.debug.demo || r.taskCode === "T3") && r.mode !== "plan" && r.role !== "PLANNER") return [...output, { kind: "error", message: r.debug.demo ? demoScript(r.debug.demoLanguage ?? "en").failure : "Debug Provider: scripted execution failure" }];
    const file = r.writeScope?.map(p => p.endsWith("/**") ? p.slice(0, -3) : p).find((p) => !p.includes("*") && !p.includes("?") && !p.includes("..") && !p.startsWith("/"));
    if (file && r.role !== "PLANNER" && r.role !== "REVIEWER" && r.mode !== "plan") {
      const text = (r.debug.demo ? demoFile(file, r.debug.demoLanguage ?? "en", r.phase === "REPAIR", r.debug.scenario) : undefined) ?? `export const debugResult = '${r.phase === "REPAIR" ? "repaired" : "success"}';\n`;
      MOCK_FILES[`${r.cwd}/${file}`] = text;
      output.push({ kind: "toolStart", id: "mock-edit", tool: "edit", title: file, detail: r.debug.demo ? demoScript(r.debug.demoLanguage ?? "en").working : "Simulated edit" }, { kind: "toolEnd", id: "mock-edit", ok: true, output: null }, { kind: "fileChange", toolId: "mock-edit", path: file, absPath: `${r.cwd}/${file}`, previewPath: null, change: "added", diff: `--- /dev/null\n+++ b/${file}\n@@ -0,0 +1 @@\n+${text}`, additions: 1, deletions: 0 });
    }
    const reply = r.debug.demo ? (r.mode === "plan" || r.role === "PLANNER" ? demoPlan(r.debug.demoLanguage ?? "en").rawMarkdown : demoReport(r.debug.demoLanguage ?? "en", r.taskCode)) : r.mode === "plan" ? mockPlanMarkdown : r.taskId != null || r.timeoutTier === "AGENT"
      ? "### What was completed\nScripted task completed.\n### Deviations from contracts\nNone\n### Hints for downstream\nUse the debug result.\n### Self-check against acceptance criteria\n- Simulated acceptance passed.\n### Defects found\nNone\n"
      : r.debug.mockReply || "Debug Provider returned a scripted response. No model API was called.";
    const chars = [...reply];
    for (let i = 0; i < chars.length; i += 32) output.push({ kind: "text", text: chars.slice(i, i + 32).join("") });
    output.push({ kind: "done", inputTokens: 0, outputTokens: 0, apiCalls: 0, contextTokens: 0, costUsd: 0, premiumRequests: 0, source: "STRUCTURED" });
    return output;
  }
  const words = (s: string) => s.split(/(\s+)/).filter(Boolean);
  const think = "The user wants the bug in `m.py` fixed. `add` returns `a - b`; it should return the sum. I'll fix it and add a test.";
  const reply =
    r.mode === "plan"
      ? `## Plan\n\n1. **Fix** \`add\` in \`m.py\` to return \`a + b\`.\n2. **Test**: add \`test_m.py\`.\n\n| Task | Domain | Depends on |\n| --- | --- | --- |\n| T1 | backend | – |\n| T2 | testing | T1 |\n\n> Request: ${r.prompt}`
      : "Fixed `add` in `m.py` (it subtracted) and added a regression test.\n\n```python\ndef add(a, b):\n    return a + b\n```\n\n- Ran `python3 -m pytest`: **1 passed**";
  const out: ChatEventKind[] = [
    { kind: "thread", id: `mock-${r.turnId}` },
    { kind: "model", name: r.model },
    ...words(think).map((text) => ({ kind: "reasoning" as const, text })),
    { kind: "toolStart", id: "t1", tool: "read", title: "m.py", detail: null },
    { kind: "toolEnd", id: "t1", ok: true, output: "def add(a, b):\n    return a - b" },
    { kind: "toolStart", id: "t2", tool: "command", title: "git status --short", detail: "Check the working tree" },
    { kind: "toolEnd", id: "t2", ok: true, output: "" },
  ];
  if (r.mode === "direct") {
    out.push(
      { kind: "toolStart", id: "t3", tool: "edit", title: "m.py, test_m.py", detail: null },
      { kind: "toolEnd", id: "t3", ok: true, output: null },
      {
        kind: "fileChange",
        toolId: "t3",
        path: "m.py",
        absPath: `${r.cwd}/m.py`,
        previewPath: null,
        change: "modified",
        diff: "--- a/m.py\n+++ b/m.py\n@@ -1,2 +1,2 @@\n def add(a, b):\n-    return a - b\n+    return a + b\n",
        additions: 1,
        deletions: 1,
      },
      {
        kind: "fileChange",
        toolId: "t3",
        path: "test_m.py",
        absPath: `${r.cwd}/test_m.py`,
        previewPath: null,
        change: "added",
        diff: "--- /dev/null\n+++ b/test_m.py\n@@ -0,0 +1,5 @@\n+from m import add\n+\n+\n+def test_add():\n+    assert add(2, 3) == 5\n",
        additions: 5,
        deletions: 0,
      },
      { kind: "toolStart", id: "t4", tool: "command", title: "python3 -m pytest -q", detail: null },
      { kind: "toolEnd", id: "t4", ok: true, output: "1 passed in 0.01s" },
    );
  }
  out.push(...words(reply).map((text) => ({ kind: "text" as const, text })));
  out.push({
    kind: "done",
    inputTokens: 12840,
    outputTokens: 412,
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    apiCalls: 1,
    contextTokens: 12840,
    costUsd: null,
    premiumRequests: null,
    source: "STRUCTURED",
  });
  return out;
}

export async function cancelChatTurn(turnId: string): Promise<void> {
  if (!inTauri) {
    const timer = mockTimers.get(turnId);
    if (timer !== undefined) {
      (typeof window !== "undefined" ? window : globalThis).clearInterval(timer);
      mockTimers.delete(turnId);
    }
    emitMockChatEnded({ turnId, cancelled: true });
    return;
  }
  await invoke("cancel_chat_turn", { turnId });
}

// Storage (docs/08). Rows are shaped by src/lib/db.ts; see src-tauri/src/storage/repo.rs.

export interface StoredSessionRow {
  id: string;
  projectId: string;
  title: string;
  state: string;
  position: number;
  archived: boolean;
  forkedFrom: string | null;
  worktree: unknown;
  updatedAt: number;
}

export interface StoredMessageRow {
  /** Explicit execution turn; task messages stay out of main chat replay. */
  attemptId?: string;
  sessionId: string;
  seq: number;
  turnIndex: number;
  provider: string | null;
  model: string | null;
  mode: string | null;
  message: unknown;
}

export interface WriteBatch {
  projects: { id: string; name: string; path: string; position: number; expanded: boolean; trusted: boolean | null }[];
  sessions: StoredSessionRow[];
  sessionChat: { sessionId: string; thread: unknown; queue: unknown }[];
  messages: StoredMessageRow[];
  settings: { key: string; value: unknown }[];
  providers: unknown[] | null;
  customPack: { url: string; loadedAt: number; pack: unknown; warnings: string[] } | null;
  clearCustomPack: boolean;
  deleteSessions: string[];
  deleteProjects: string[];
}

export interface StoredSnapshot {
  projects: unknown[];
  conversations: Record<string, unknown>;
  settings: Record<string, unknown>;
  providers: unknown[];
  customPack: WriteBatch["customPack"];
}

export const storageLoad = () => invoke<StoredSnapshot>("storage_load");
export const storageWrite = (batch: WriteBatch) => invoke<void>("storage_write", { batch });
export const storageImport = (batch: WriteBatch) => invoke<void>("storage_import", { batch });

// Command lists (docs/11 §3). Mirrors `tokage_core::command_policy`.
export type CommandLevel = "read_only" | "project" | "sensitive" | "forbidden";

export interface PackCommandPolicy {
  read_only: string[];
  sensitive: string[];
  forbidden: string[];
  forbidden_paths: string[];
}

export interface CommandRule {
  id?: number;
  /** Null: every project. */
  projectId?: string | null;
  /** Null: every CLI. */
  provider?: ProviderId | null;
  pattern: string;
  level: CommandLevel;
  source: "digest" | "user" | "approval";
}

export interface CommandPolicy {
  pack: PackCommandPolicy;
  rules: CommandRule[];
}

export type CommandRuleChange =
  | { op: "upsert"; projectId?: string | null; provider?: ProviderId | null; pattern: string; level: CommandLevel }
  | { op: "delete"; id: number };

export interface CommandReason {
  level: CommandLevel;
  code: string;
  segment: string;
  detail?: string | null;
}

export interface CommandVerdict {
  level: CommandLevel;
  reasons: CommandReason[];
  /** False when part of the command could not be read statically. */
  resolved: boolean;
  writes: string[];
  recoverable: boolean | null;
}

export interface ClassifyRequest {
  command: string;
  cwd: string;
  workspaceRoots?: string[];
  writeScope?: string[] | null;
  projectId?: string | null;
  provider?: ProviderId | null;
  defaultBranch?: string | null;
  currentBranch?: string | null;
}

export async function getCommandPolicy(): Promise<CommandPolicy> {
  if (!inTauri) return { pack: { read_only: [], sensitive: [], forbidden: [], forbidden_paths: [] }, rules: [] };
  return invoke<CommandPolicy>("get_command_policy");
}

/** Applies one rule change and returns all rules. Rejects rules that would allow a forbidden command. */
export async function updateCommandRules(change: CommandRuleChange): Promise<CommandRule[]> {
  if (!inTauri) throw new Error("Command rules need the desktop app");
  return invoke<CommandRule[]>("update_command_rules", { change });
}

// Authorization modes (docs/11 §2). Mirrors `tokage_core::auth`.
export type AuthMode = "ask_always" | "ask_sensitive" | "full_auto";
export type AuthRole = "PLANNER" | "EXECUTOR" | "TESTER" | "REVIEWER";
export type AuthScope = "global" | "project" | "session";

export interface AuthLayers {
  global: AuthMode;
  project: AuthMode | null;
  session: AuthMode | null;
}

export interface Authorization {
  verdict: CommandVerdict;
  mode: { mode: AuthMode; scope: AuthScope; fullAutoUnavailable: boolean };
  outcome: AuthOutcome;
  /** The same verdict under each mode. */
  byMode: Record<AuthMode, AuthOutcome>;
}

export interface AuthOutcome {
  decision: "allow" | "ask" | "deny";
  /** `forbidden`, `read_only`, `project`, `sensitive`, `ask_always`, `full_auto`, `unresolved`, `read_only_role`, `tester_role`. */
  reason: string;
  recoverable?: boolean | null;
}

export interface AuthorizeRequest extends ClassifyRequest {
  /** `command` is a file path written by the CLI's edit tool. */
  edit?: boolean;
  /** `command` is a file path read by the CLI's read tool. */
  read?: boolean;
  role?: AuthRole;
  layers: AuthLayers;
}

/** The mode's decision for a command or file edit; `full_auto` needs a sandboxed CLI. */
export async function authorizeCommand(request: AuthorizeRequest): Promise<Authorization> {
  if (!inTauri) throw new Error("Command classification needs the desktop app");
  return invoke<Authorization>("authorize_command", { request });
}

export type UserDecision = "allow_once" | "allow_session" | "allow_project" | "deny";

export interface ApprovalRecord {
  id?: number | null;
  attemptId?: string | null;
  sessionId?: string | null;
  turnId?: string | null;
  command: string;
  level: string;
  reason?: string | null;
  decision: string;
  mode: string;
  decidedAt?: string | null;
}

export interface ResolveApprovalRequest {
  id: string;
  sessionId?: string | null;
  turnId?: string | null;
  attemptId?: string | null;
  toolId?: string | null;
  command: string;
  decision: UserDecision;
  pattern?: string | null;
  projectId?: string | null;
  mode?: string | null;
}

export async function listApprovals(sessionId?: string | null): Promise<ApprovalRecord[]> {
  if (!inTauri) return [];
  return invoke<ApprovalRecord[]>("list_approvals", { sessionId });
}

export async function resolveApproval(request: ResolveApprovalRequest): Promise<void> {
  if (!inTauri) return;
  await invoke("resolve_approval", { request });
}

export interface UsageEventRow {
  id?: number | null;
  attemptId?: string | null;
  taskId?: number | null;
  planId?: number | null;
  providerName: string;
  modelId?: string | null;
  role?: string | null;
  domain?: string | null;
  turnIndex?: number | null;
  inputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  outputTokens: number;
  apiCalls?: number | null;
  contextTokens?: number | null;
  gapSeconds?: number | null;
  credits?: number | null;
  costUsd?: number | null;
  equivCostUsd?: number | null;
  phase: "PLANNING" | "EXECUTION" | "REPAIR" | "CONTEXT" | "DIRECT";
  source: "STRUCTURED" | "STDOUT_REGEX" | "ESTIMATED";
  createdAt?: string | null;
}

// ---------------------------------------------------------------------------------------
// Network Proxy (docs/12, P3.m6)

export type ProxyMode = "inherit_global" | "cli_own" | "none" | "system" | "manual";
export type ProxyProtocol = "http" | "https" | "socks5";

export interface ProxyUpstream {
  protocol: ProxyProtocol;
  host: string;
  port: number;
  username?: string | null;
  password?: string | null;
}

export interface CliProxyConfig {
  mode: ProxyMode;
  forced: boolean;
  upstream?: ProxyUpstream | null;
}

export interface GlobalProxyConfig {
  mode: ProxyMode;
  upstream?: ProxyUpstream | null;
  no_proxy: string;
  ca_file?: string | null;
  local_relay: boolean;
  per_cli: Record<string, CliProxyConfig>;
}

export interface ProviderProxyStatus {
  provider: ProviderId;
  env_vars: string[];
  extra_env: Record<string, string>;
  own_config_proxy?: string | null;
  supports_socks: boolean;
  forced_supported: boolean;
  verified: boolean;
}

export interface ProxyTestResult {
  success: boolean;
  endpoint: string;
  latency_ms: number;
  message?: string | null;
}

export interface SystemProxyResponse {
  upstream: ProxyUpstream | null;
  noProxy: string | null;
}

export async function getProxyConfig(): Promise<GlobalProxyConfig> {
  if (!inTauri) {
    return {
      mode: "cli_own",
      upstream: null,
      no_proxy: "localhost,127.0.0.1,::1",
      ca_file: null,
      local_relay: false,
      per_cli: {},
    };
  }
  return invoke<GlobalProxyConfig>("get_proxy_config");
}

export async function saveProxyConfig(config: GlobalProxyConfig): Promise<void> {
  if (!inTauri) return;
  await invoke("save_proxy_config", { config });
}

export async function getSystemProxy(): Promise<SystemProxyResponse> {
  if (!inTauri) return { upstream: null, noProxy: null };
  return invoke<SystemProxyResponse>("get_system_proxy");
}

export async function testProxyConnection(
  provider?: ProviderId,
  endpoint?: string,
  config?: GlobalProxyConfig
): Promise<ProxyTestResult> {
  if (!inTauri) {
    return {
      success: true,
      endpoint: endpoint || "https://api.openai.com",
      latency_ms: 120,
      message: "HTTP 200 (mock)",
    };
  }
  return invoke<ProxyTestResult>("test_proxy_connection", { provider, endpoint, config });
}

export async function getAllProviderProxyStatuses(): Promise<ProviderProxyStatus[]> {
  if (!inTauri) return [];
  return invoke<ProviderProxyStatus[]>("get_all_provider_proxy_statuses");
}

export interface NativeSession {
  id: string;
  providerId: ProviderId;
  title: string;
  cwd: string;
  updatedAt: string;
  fileSize: number;
  path: string;
}

export async function listNativeSessions(
  provider?: ProviderId,
  projectPath?: string
): Promise<NativeSession[]> {
  if (!inTauri) {
    return [
      {
        id: "mock-session-1",
        providerId: "CLAUDE_CODE",
        title: "Mock External Claude Session",
        cwd: projectPath || "/Users/mock/project",
        updatedAt: new Date().toISOString(),
        fileSize: 10240,
        path: "/mock/.claude/projects/session.jsonl",
      },
    ];
  }
  return invoke<NativeSession[]>("list_native_sessions", {
    provider: provider ?? null,
    projectPath: projectPath ?? null,
  });
}

// ---------------------------------------------------------------------------
// Planning Engine (02, P5.m1 / P5.m2)
// ---------------------------------------------------------------------------

export interface ProjectCommands {
  build: string[];
  test: string[];
  check: string[];
  lint: string[];
}

export interface ManifestSummary {
  manifestType: string;
  path: string;
  name?: string;
  version?: string;
  packageManager?: string;
  dependencies: string[];
  devDependencies: string[];
  scripts: Record<string, string>;
}

export interface HotspotItem {
  path: string;
  commitCount: number;
}

export interface RepoDigest {
  projectPath: string;
  generatedAt: string;
  primaryLanguages: string[];
  manifests: ManifestSummary[];
  commands: ProjectCommands;
  conventionsSummary: string;
  treeSummary: string;
  publicSymbols: string[];
  hotspots: HotspotItem[];
  estimatedTokens: number;
  markdown: string;
}

export type TaskSize = "S" | "M" | "L";

export interface TaskAcceptance {
  commands: string[];
  criteria: string[];
  /** Commands the user confirmed outside the pack allowlist (F5.c). */
  confirmedCommands?: string[];
}

export interface TaskItem {
  id: string;
  domain: string;
  title: string;
  goal: string;
  depends_on: string[];
  contracts: string[];
  write_scope: string[];
  deliverables: string[];
  acceptance: TaskAcceptance;
  size: TaskSize;
  difficulty: number;
  assigned_model?: string | null;
  /** Flags `assigned_model`'s id can't carry (CLIs that take effort / fast / tier as parameters). */
  model_params?: ModelParams | null;
}

/** Parameters stored beside a task's model id. */
export interface ModelParams {
  effort?: string | null;
  fast?: boolean;
  tier?: string | null;
}

/** A model that can run right now; the allocator only considers these. */
export interface AvailableModel {
  provider: string;
  cliModelArg: string;
}

export interface PlanSpec {
  raw: string;
  goal: string;
  inScope: string[];
  outOfScope: string[];
  decisions: string[];
}

export interface PlanContract {
  id: string;
  title: string;
  content: string;
}

export interface Plan {
  version?: number;
  parentPlanId?: string | null;
  spec: PlanSpec;
  contracts: PlanContract[];
  tasks: TaskItem[];
  notes?: string;
  rawMarkdown: string;
  isUserEdited?: boolean | null;
}

export async function getRepoDigest(
  path: string,
  maxTokens?: number
): Promise<RepoDigest> {
  if (!inTauri) {
    return {
      projectPath: path,
      generatedAt: new Date().toISOString(),
      primaryLanguages: ["TypeScript", "Rust"],
      manifests: [],
      commands: {
        build: ["cargo build", "pnpm build"],
        test: ["cargo test", "pnpm test"],
        check: ["pnpm check"],
        lint: ["pnpm lint"],
      },
      conventionsSummary: "",
      treeSummary: "",
      publicSymbols: [],
      hotspots: [],
      estimatedTokens: 100,
      markdown: "# Mock Repo Digest",
    };
  }
  return invoke<RepoDigest>("get_repo_digest", {
    path,
    maxTokens: maxTokens ?? null,
  });
}

export async function renderPlanningPrompt(args: {
  userRequest: string;
  repoDigest: string;
  domains?: string[];
  outputMaxTokens?: number;
  customFormatSpec?: string;
}): Promise<string> {
  if (!inTauri) {
    return `You are a technical planner...\n# Project digest\n${args.repoDigest}\n# Request\n${args.userRequest}\n# Spec`;
  }
  return invoke<string>("render_planning_prompt", {
    userRequest: args.userRequest,
    repoDigest: args.repoDigest,
    domains: args.domains ?? null,
    outputMaxTokens: args.outputMaxTokens ?? null,
    customFormatSpec: args.customFormatSpec ?? null,
  });
}

/** F16.b: format-repair prompt from PLAN_FORMAT_SPEC. */
export async function renderPlanRepairPrompt(rawText: string): Promise<string> {
  if (!inTauri) {
    return `Convert the following technical plan into Markdown that strictly follows the Tokage format:\n\n${rawText}`;
  }
  return invoke<string>("render_plan_repair_prompt", { rawText });
}

export async function parsePlanningOutput(args: {
  markdown: string;
  defaultTestCommands?: string[];
  defaultDomain?: string;
}): Promise<Plan> {
  if (!inTauri) {
    for (const language of ["en", "zh"] as const) {
      if (args.markdown.includes(demoScript(language).plan.spec.goal)) return demoPlan(language);
    }
    if (debugConfig().output === "mock" && args.markdown.includes("Exercise the complete Tokage pipeline")) return (await import("./debugMock")).debugMockPlan();
    return {
      spec: {
        raw: "",
        goal: "Mock Goal",
        inScope: [],
        outOfScope: [],
        decisions: [],
      },
      contracts: [],
      tasks: [
        {
          id: "T1",
          domain: args.defaultDomain || "general",
          title: "Mock Task",
          goal: "Execute mock task",
          depends_on: [],
          contracts: [],
          write_scope: [],
          deliverables: [],
          acceptance: {
            commands: args.defaultTestCommands || [],
            criteria: [],
          },
          size: "M",
          difficulty: 2,
        },
      ],
      rawMarkdown: args.markdown,
    };
  }
  return invoke<Plan>("parse_planning_output", {
    markdown: args.markdown,
    defaultTestCommands: args.defaultTestCommands ?? null,
    defaultDomain: args.defaultDomain ?? null,
  });
}

export type ValidationSeverity = "info" | "warning" | "error";

export interface ValidationIssue {
  severity: ValidationSeverity;
  rule: string;
  taskId?: string | null;
  /** English fallback text; use `localizeIssue` for the UI. */
  message: string;
  /** Positional values for the localized message of `rule`. */
  params?: string[];
}

export interface ValidationResult {
  isValid: boolean;
  issues: ValidationIssue[];
  plan: Plan;
}

export interface PersistPlanResult {
  planId: string;
  planDir: string;
  filesWritten: string[];
}

export async function validatePlan(args: {
  plan: Plan;
  maxTasks?: number;
  validDomains?: string[];
  allowedCommands?: string[];
  autoSerializeConflicts?: boolean;
}): Promise<ValidationResult> {
  if (!inTauri) {
    return {
      isValid: true,
      issues: [],
      plan: args.plan,
    };
  }
  return invoke<ValidationResult>("validate_plan", {
    plan: args.plan,
    maxTasks: args.maxTasks ?? null,
    validDomains: args.validDomains ?? null,
    allowedCommands: args.allowedCommands ?? null,
    autoSerializeConflicts: args.autoSerializeConflicts ?? null,
  });
}

export async function persistPlan(
  cwd: string,
  planId: string,
  plan: Plan
): Promise<PersistPlanResult> {
  if (!inTauri) {
    if (demoIdentity(planId)) browserDemoPlans.set(planId, structuredClone(plan));
    return {
      planId,
      planDir: `${cwd}/.tokage/plans/${planId}`,
      filesWritten: ["spec.md", "tasks.yaml", "plan.md"],
    };
  }
  return invoke<PersistPlanResult>("persist_plan", {
    cwd,
    planId,
    plan,
  });
}

export async function persistPlanUserEdited(
  cwd: string,
  planId: string,
  plan: Plan
): Promise<PersistPlanResult> {
  if (!inTauri) {
    if (demoIdentity(planId)) browserDemoPlans.set(planId, structuredClone(plan));
    return {
      planId,
      planDir: `${cwd}/.tokage/plans/${planId}/user_edited`,
      filesWritten: ["user_edited/spec.md", "user_edited/tasks.yaml", "user_edited/plan.md"],
    };
  }
  return invoke<PersistPlanResult>("persist_plan_user_edited", {
    cwd,
    planId,
    plan,
  });
}

export async function resetPlanToDefault(
  cwd: string,
  planId: string
): Promise<Plan> {
  if (!inTauri) {
    const identity = demoIdentity(planId);
    if (identity) { const plan = demoPlan(identity.language); browserDemoPlans.set(planId, plan); return structuredClone(plan); }
    return {
      version: 1,
      spec: { raw: "", goal: "Restored Default", inScope: [], outOfScope: [], decisions: [] },
      contracts: [],
      tasks: [],
      rawMarkdown: "",
      isUserEdited: false,
    };
  }
  return invoke<Plan>("reset_plan_to_default", {
    cwd,
    planId,
  });
}

export interface ContractDiff {
  id: string;
  title: string;
  action: "added" | "modified" | "removed" | "unchanged";
  oldContent?: string | null;
  newContent?: string | null;
}

export interface TaskDiff {
  id: string;
  title: string;
  action: "added" | "modified" | "removed" | "unchanged";
  oldTask?: TaskItem | null;
  newTask?: TaskItem | null;
  contractsChanged: boolean;
  dependenciesChanged: boolean;
  scopeChanged: boolean;
}

export interface SpecDiff {
  goalChanged: boolean;
  oldGoal: string;
  newGoal: string;
  inScopeAdded: string[];
  inScopeRemoved: string[];
  outOfScopeAdded: string[];
  outOfScopeRemoved: string[];
  decisionsAdded: string[];
  decisionsRemoved: string[];
}

export interface PlanDiff {
  fromVersion: number;
  toVersion: number;
  specDiff: SpecDiff;
  contracts: ContractDiff[];
  tasks: TaskDiff[];
  notesChanged: boolean;
}

export async function computePlanDiff(
  currentPlan: Plan,
  proposedPlan: Plan
): Promise<PlanDiff> {
  if (!inTauri) {
    return {
      fromVersion: currentPlan.version ?? 1,
      toVersion: (currentPlan.version ?? 1) + 1,
      specDiff: {
        goalChanged: currentPlan.spec.goal !== proposedPlan.spec.goal,
        oldGoal: currentPlan.spec.goal,
        newGoal: proposedPlan.spec.goal,
        inScopeAdded: [],
        inScopeRemoved: [],
        outOfScopeAdded: [],
        outOfScopeRemoved: [],
        decisionsAdded: [],
        decisionsRemoved: [],
      },
      contracts: [],
      tasks: [],
      notesChanged: false,
    };
  }
  return invoke<PlanDiff>("compute_plan_diff", {
    currentPlan,
    proposedPlan,
  });
}

export async function applyPlanDiff(
  currentPlan: Plan,
  diff: PlanDiff,
  proposedPlan: Plan,
  parentPlanId?: string | null
): Promise<Plan> {
  if (!inTauri) {
    return {
      ...proposedPlan,
      version: diff.toVersion,
      parentPlanId: parentPlanId ?? null,
    };
  }
  return invoke<Plan>("apply_plan_diff", {
    currentPlan,
    diff,
    proposedPlan,
    parentPlanId,
  });
}

export type ReplanTaskStatus =
  | "PENDING"
  | "READY"
  | "RUNNING"
  | "VERIFYING"
  | "REPAIRING"
  | "DONE"
  | "FAILED"
  | "CANCELLED"
  | "SKIPPED";

export interface TaskExecutionReport {
  taskId: string;
  status: ReplanTaskStatus;
  completedSummary: string;
  fullReport?: string | null;
  modifiedFiles?: string[];
}

export async function assembleReplanPrompt(args: {
  currentPlan: Plan;
  changeDescription: string;
  taskReports: TaskExecutionReport[];
  affectedContractIds?: string[];
  domains?: string[];
  replanInputMaxTokens?: number;
}): Promise<string> {
  if (!inTauri) {
    return `INCREMENTAL REPLAN: ${args.changeDescription}`;
  }
  return invoke<string>("assemble_replan_prompt", {
    currentPlan: args.currentPlan,
    changeDescription: args.changeDescription,
    taskReports: args.taskReports,
    affectedContractIds: args.affectedContractIds,
    domains: args.domains,
    replanInputMaxTokens: args.replanInputMaxTokens,
  });
}

export async function generateAdaptationTasks(args: {
  currentPlan: Plan;
  changedContractIds: string[];
  taskReports: TaskExecutionReport[];
}): Promise<TaskItem[]> {
  if (!inTauri) {
    return [];
  }
  return invoke<TaskItem[]>("generate_adaptation_tasks", {
    currentPlan: args.currentPlan,
    changedContractIds: args.changedContractIds,
    taskReports: args.taskReports,
  });
}

export async function loadPlan(
  cwd: string,
  planId: string,
  version?: number | null
): Promise<Plan> {
  if (!inTauri) {
    const identity = demoIdentity(planId);
    if (identity) return structuredClone(browserDemoPlans.get(planId) ?? demoPlan(identity.language));
    return {
      version: version ?? 1,
      spec: { raw: "", goal: "Mock", inScope: [], outOfScope: [], decisions: [] },
      contracts: [],
      tasks: [],
      rawMarkdown: "",
    };
  }
  return invoke<Plan>("load_plan", {
    cwd,
    planId,
    version,
  });
}

export type TriageRoute = "PLAN" | "DIRECT";

export interface TriageRuleMatch {
  ruleId: string;
  toward: TriageRoute;
  weight: number;
  reason: string;
}

export interface TriageResult {
  route: TriageRoute;
  confidence: number;
  detectedDomains: string[];
  matchedRules: TriageRuleMatch[];
  estimatedFiles: string[];
  scorePlan: number;
  scoreDirect: number;
  reason: string;
}

export async function triageRequest(
  prompt: string,
  projectPath?: string
): Promise<TriageResult> {
  if (!inTauri) {
    const isPlan = /实现|开发|搭建|重构|架构|implement|build|refactor/i.test(prompt);
    return {
      route: isPlan ? "PLAN" : "DIRECT",
      confidence: 0.8,
      detectedDomains: [],
      matchedRules: [],
      estimatedFiles: [],
      scorePlan: isPlan ? 2 : 0,
      scoreDirect: isPlan ? 0 : 2,
      reason: isPlan ? "Mock fallback: PLAN" : "Mock fallback: DIRECT",
    };
  }
  return invoke<TriageResult>("triage_request", {
    prompt,
    projectPath: projectPath ?? null,
  });
}

export interface AlternativeCandidate {
  provider: string;
  modelId: string;
  displayName: string;
  pSuccess: number;
  estCostUsd: number;
  meetsThreshold: boolean;
}

export interface TaskAssignment {
  taskId: string;
  provider: string;
  modelId: string;
  displayName: string;
  cliModelArg: string;
  pSuccess: number;
  estCostUsd: number;
  meetsThreshold: boolean;
  reuseAttemptId?: string | null;
  explanation: string;
  alternatives: AlternativeCandidate[];
}

export interface PlanAllocationResult {
  assignments: TaskAssignment[];
  totalEstCostUsd: number;
  allMeetThreshold: boolean;
  providerUtilizations: Record<string, number>;
}

export async function allocatePlanModels(
  plan: Plan,
  manualOverrides?: Record<string, string>,
  initialProviderUtils?: Record<string, number>,
  availableModels?: AvailableModel[],
): Promise<PlanAllocationResult> {
  if (!inTauri) {
    const assignments: TaskAssignment[] = plan.tasks.map((t) => ({
      taskId: t.id,
      provider: "CLAUDE_CODE",
      modelId: "claude-sonnet-5-5",
      displayName: "Claude Sonnet 5.5",
      cliModelArg: "sonnet",
      pSuccess: 0.88,
      estCostUsd: 0.22,
      meetsThreshold: true,
      explanation: i18n.t("mock.assign.sonnet", { domain: t.domain, difficulty: t.difficulty, size: t.size }),
      alternatives: [],
    }));
    return {
      assignments,
      totalEstCostUsd: assignments.reduce((acc, a) => acc + a.estCostUsd, 0),
      allMeetThreshold: true,
      providerUtilizations: { CLAUDE_CODE: 0.1 },
    };
  }
  // F15.e: seed scarcity from live utilization when caller didn't pass utils.
  let utils = initialProviderUtils ?? null;
  if (!utils) {
    try {
      const providers = [...new Set(plan.tasks.map((t) => t.assigned_model?.split(":")[0]).filter(Boolean))] as ProviderId[];
      // Also probe common providers from health list when plan has no assignments yet.
      const health = await listProviderHealth().catch(() => []);
      const ids = new Set<ProviderId>([
        ...providers,
        ...health.map((h) => h.providerName as ProviderId),
      ]);
      const built: Record<string, number> = {};
      for (const id of ids) {
        if (!id) continue;
        try {
          const u = await getProviderUtilizationNow(id, 0);
          if (u.utilizationPct != null && Number.isFinite(u.utilizationPct)) built[id] = u.utilizationPct / 100;
        } catch {
          /* skip */
        }
      }
      if (Object.keys(built).length > 0) utils = built;
    } catch {
      /* best-effort */
    }
  }
  return invoke<PlanAllocationResult>("allocate_plan_models", {
    plan,
    manualOverrides: manualOverrides ?? null,
    initialProviderUtils: utils,
    availableModels: availableModels ?? null,
  });
}

export async function assignDirectTask(
  domain: string,
  difficulty: number,
  size?: TaskSize,
  initialProviderUtils?: Record<string, number>,
  availableModels?: AvailableModel[],
): Promise<TaskAssignment | null> {
  if (!inTauri) {
    return {
      taskId: "DIRECT",
      provider: "CLAUDE_CODE",
      modelId: "claude-haiku-4-5",
      displayName: "Claude Haiku 4.5",
      cliModelArg: "haiku",
      pSuccess: 0.90,
      estCostUsd: 0.05,
      meetsThreshold: true,
      explanation: i18n.t("mock.assign.haiku", { domain, difficulty, size: size ?? "S" }),
      alternatives: [],
    };
  }
  return invoke<TaskAssignment | null>("assign_direct_task", {
    domain,
    difficulty,
    size: size ?? null,
    initialProviderUtils: initialProviderUtils ?? null,
    availableModels: availableModels ?? null,
  });
}

/** F16.d: baseline cost if the DIRECT task ran on the reference model. */
export async function calculateDirectCounterfactualCost(args: {
  domain: string;
  difficulty: number;
  size?: TaskSize;
  referenceModel?: string;
}): Promise<number> {
  if (!inTauri) return 0.25;
  return invoke<number>("calculate_direct_counterfactual_cost", {
    domain: args.domain,
    difficulty: args.difficulty,
    size: args.size ?? null,
    referenceModel: args.referenceModel ?? null,
  });
}

export type TaskOutcomeResult =
  | "FIRST_PASS"
  | "FIXED_IN_WINDOW"
  | "REASSIGNED"
  | "BLOCKED"
  | "USER_TAKEOVER";

export interface TaskOutcomeRow {
  id?: number | null;
  taskId?: string | null;
  modelId: string;
  providerName: string;
  domain: string;
  difficulty: number;
  size: string;
  result: TaskOutcomeResult;
  pPredicted: number;
  estCostUsd?: number | null;
  actualCostUsd?: number | null;
  turns?: number | null;
  createdAt?: string | null;
}

export interface LearnedStatRow {
  providerName: string;
  modelId: string;
  domain: string;
  bucket: string;
  metric: string;
  value: number;
  samples: number;
  updatedAt?: string | null;
}

export async function recordTaskOutcome(
  outcome: TaskOutcomeRow,
  attemptId?: string | null,
): Promise<number> {
  if (attemptId?.includes("tokage-demo-") || (debugTurnConfig(attemptId)?.output ?? debugConfig().output) !== "off") return 0;
  if (!inTauri) {
    return 1;
  }
  return invoke<number>("record_task_outcome", {
    outcome,
    attemptId: attemptId ?? null,
  });
}

export async function listTaskOutcomes(
  modelId?: string,
  domain?: string,
  limit?: number
): Promise<TaskOutcomeRow[]> {
  if (!inTauri) {
    return [];
  }
  return invoke<TaskOutcomeRow[]>("list_task_outcomes", {
    modelId: modelId ?? null,
    domain: domain ?? null,
    limit: limit ?? null,
  });
}

export async function listLearnedStats(
  modelId: string
): Promise<LearnedStatRow[]> {
  if (!inTauri) {
    return [];
  }
  return invoke<LearnedStatRow[]>("list_learned_stats", { modelId });
}

/** F14.f */
export async function upsertModelOverride(
  providerName: string,
  modelId: string,
  patch: unknown,
): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("upsert_model_override", {
    providerName,
    modelId,
    patchJson: JSON.stringify(patch),
  });
}

export async function listModelOverrides(): Promise<
  { providerName: string; modelId: string; patchJson: string; updatedAt?: string }[]
> {
  if (!inTauri) return [];
  return invoke("list_model_overrides");
}

export async function deleteModelOverride(
  providerName: string,
  modelId: string,
): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("delete_model_override", { providerName, modelId });
}

export async function replaceCustomPacks(
  packs: {
    name: string;
    url?: string;
    pack: unknown;
    warnings?: string[];
    enabled: boolean;
  }[],
): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("replace_custom_packs_cmd", { packs });
}

export async function listCustomPacks(): Promise<
  { name: string; pack: unknown; position: number; enabled: boolean }[]
> {
  if (!inTauri) return [];
  return invoke("list_custom_packs_cmd");
}

export type TaskState =
  | "PENDING"
  | "READY"
  | "RUNNING"
  | "VERIFYING"
  | "DONE"
  | "REPAIRING"
  | "BLOCKED"
  | "CANCELLED"
  | "SKIPPED"
  | "PAUSED_QUOTA"
  | "WAITING_UPSTREAM"
  | "INTERRUPTED";

export type PipelineStatus = "RUNNING" | "PAUSED" | "DONE" | "INTERRUPTED";

/** Per-task worktree metadata persisted in pipeline state (F11 / F7). */
export interface TaskWorktreeInfo {
  root: string;
  cwd: string;
  branch: string;
  baseCommit: string;
}

export interface IntegrationInfo {
  branch: string;
  root: string;
  cwd?: string | null;
  target?: CheckoutIdentity | null;
  /** Project HEAD when the integration worktree was created (F11.f export base). */
  baseCommit?: string | null;
}

export interface PipelineTaskInfo {
  id: string;
  title: string;
  state: string;
  assignedModel?: string | null;
  modelParams?: ModelParams | null;
  turnId?: string | null;
  startedAt?: number | null;
  endedAt?: number | null;
  /** F7.b */
  provider?: string | null;
  threadId?: string | null;
  cwd?: string | null;
  worktree?: TaskWorktreeInfo | null;
  repairStep?: string | null;
  allowedCommands?: string[];
  approvals?: unknown[];
  costUsd?: number | null;
}

export interface PipelineGateResult {
  status?: "passed" | "failed" | "error" | "cancelled" | "not_run";
  taskId: string;
  command: string;
  passed: boolean;
  exitCode?: number | null;
  durationMs: number;
  outputTail: string;
}

/** Durable delivery outcome; in-flight gate runs are restarted after recovery. */
export type DeliveryPhase =
  | "idle"
  | "running_gates"
  | "ready"
  | "merged"
  | "exported"
  | "kept"
  | "failed";

export interface PlanDeliveryState {
  phase: DeliveryPhase;
  intent?: { kind: "merge" | "export" | "keep"; source: { branch: string; commit: string; baseCommit: string }; patchPath?: string };
  summary?: DeliverySummary | null;
  fullGatePassed?: boolean;
  acceptance?: "passed" | "failed" | "not_configured" | "cancelled" | "stale";
  gateResults?: PipelineGateResult[];
  evidence?: { snapshot: ScopeSnapshot; plan?: string; commands: string[]; taskStates: Record<string, TaskState>; tasks: string };
  acceptedAt?: number;
  patchPath?: string | null;
  source?: { branch: string; commit: string; baseCommit: string };
  error?: string | null;
}

/** Durable intent written before any task Git mutation (m1d). */
export interface TaskGitDelivery {
  phase: "committing" | "committed" | "merged" | "conflict";
  worktree: TaskWorktreeInfo;
  integration: IntegrationInfo;
  scope: { snapshot: ScopeSnapshot; changedFiles: string[]; checkedAt: number };
  sourceCommit?: string;
  outcome?: WorktreeMergeOutcome;
  preparation?: TaskWorktreeInfo;
  preparationName?: string;
}

export interface CompletionReceipt {
  turnId: string; taskIdentity: string; planDefinition?: string;
  rawOutput: string; changedFiles: string[]; workdir: string; costUsd?: number | null;
}

export interface PipelineState {
  persistenceRevision?: number;
  delivery?: PlanDeliveryState | null;
  taskAttemptCounts?: Record<string, number>;
  /** 2 for new writes; missing → treat as 1. */
  schemaVersion?: number;
  sessionId: string;
  status: PipelineStatus;
  planDbId?: number | null;
  taskDbIds?: Record<string, number>;
  completionReceipts?: Record<string, CompletionReceipt>;
  recoveryTasks?: string[];
  gitDeliveries?: Record<string, TaskGitDelivery>;
  taskFailureEvidence?: Record<string, FailureEvidence & { workdir: string; turnId?: string }>;
  scopeBaselines?: Record<string, ScopeSnapshot>;
  scopeChecks?: Record<string, { snapshot: ScopeSnapshot; changedFiles: string[]; checkedAt: number }>;
  /** Explicit origin for unchanged completed tasks inherited by a replan. */
  taskReportPlanIds?: Record<string, number>;
  planDefinition?: string | null;
  baseCwd?: string | null;
  integration?: IntegrationInfo | null;
  tasks: PipelineTaskInfo[];
  /** Full TaskItem definitions (v2). Empty/missing on v1. */
  taskItems?: TaskItem[];
  contracts?: PlanContract[];
  upstreamArtifacts?: Record<string, UpstreamArtifact>;
  assignments?: Record<string, TaskAssignment>;
  deviationSources?: string[];
  taskStates: Record<string, string>;
  taskLogs: Record<string, string[]>;
  gateResults: PipelineGateResult[];
  startedAt: number;
  updatedAt?: number | null;
}

export async function savePipelineState(cwd: string, state: PipelineState, expectedRevision?: number): Promise<void> {
  if (!inTauri) {
    return;
  }
  return invoke<void>("save_pipeline_state_cmd", { cwd, state, expectedRevision: expectedRevision ?? null });
}

export async function taskRecoveryEvidence(turnId: string, cwd: string, provider?: string, threadId?: string): Promise<{
  runtimeIdle: boolean; launchRecorded: boolean; nativeSessionPresent: boolean | null;
}> {
  if (!inTauri) return { runtimeIdle: true, launchRecorded: false, nativeSessionPresent: null };
  return invoke("task_recovery_evidence", { turnId, cwd, provider: provider ?? null, threadId: threadId ?? null });
}

export async function loadPipelineState(cwd: string, sessionId: string): Promise<PipelineState | null> {
  if (!inTauri) {
    return null;
  }
  return invoke<PipelineState | null>("load_pipeline_state_cmd", { cwd, sessionId });
}

export interface ConcurrencyLimits {
  maxGlobalConcurrent: number;
  defaultProviderConcurrent: number;
  providerOverrides?: Record<string, number>;
}

/** F9 / A.10 — reason a READY (or promoted) task cannot start this round. */
export type DispatchIneligibleReason =
  | { globalSlotsFull: { current: number; max: number } }
  | { providerSlotsFull: { provider: string; current: number; max: number } }
  | { writeScopeCollision: { conflictingTaskId: string; overlappingPaths: readonly string[] } }
  | { contractDeviationPaused: { sourceTaskId: string } };

export interface PickDispatchInput {
  tasks: TaskItem[];
  taskStates: Record<string, TaskState>;
  /** taskId → [provider, model] */
  assignments: Record<string, [string, string]>;
  maxGlobalConcurrent: number;
  deviationSources: string[];
}

export interface PickDispatchOutput {
  promoted: string[];
  dispatchable: string[];
  ineligible: Record<string, DispatchIneligibleReason>;
}

export interface UpstreamArtifact {
  taskId: string;
  title: string;
  report: string;
  changedFiles: string[];
}

export interface TaskBriefInput {
  task: TaskItem;
  contracts: PlanContract[];
  upstreamArtifacts: UpstreamArtifact[];
  failureEvidence?: string | null;
}

export interface TaskReport {
  taskId: string;
  title: string;
  model: string;
  attemptId: string;
  firstPass: boolean;
  isLocallyGenerated: boolean;
  completedWork: string;
  contractDeviations?: string | null;
  tipsForDownstream: string;
  selfCheckCriteria: string[];
  changedFilesStat: string;
  changedFilesList: string[];
  gateResultsSummary: string;
  costUsd?: number | null;
  /** F13.e: testing-task defect notes (`- <implTaskId>: <desc>`). */
  defects?: string | null;
}

export interface TaskReportRow {
  sessionId: string;
  planRecordId: number;
  id?: number | null;
  taskId: string;
  modelId: string;
  attemptId: string;
  firstPass: boolean;
  isLocallyGenerated: boolean;
  summaryMd: string;
  fullMd: string;
  contractDeviation?: string | null;
  changedFilesStat: string;
  changedFilesList: string[];
  gateResults: string;
  costUsd?: number | null;
  createdAt?: string | null;
}

/**
 * F9: ask Rust DagScheduler which tasks to promote / start / wait on.
 * Browser mock applies dep promotion + global concurrency only (no provider/scope).
 */
export async function pickDispatchableTasks(
  input: PickDispatchInput,
): Promise<PickDispatchOutput> {
  if (!inTauri) {
    const maxGlobal = Math.max(1, input.maxGlobalConcurrent || 1);
    const states = { ...input.taskStates };
    const promoted: string[] = [];
    for (const t of input.tasks) {
      if (states[t.id] !== "PENDING") continue;
      if (t.depends_on.every((d) => states[d] === "DONE")) {
        promoted.push(t.id);
        states[t.id] = "READY";
      }
    }
    const deviation = new Set(input.deviationSources ?? []);
    const dependents = new Map<string, string[]>();
    for (const t of input.tasks) {
      for (const d of t.depends_on) {
        const list = dependents.get(d) ?? [];
        list.push(t.id);
        dependents.set(d, list);
      }
    }
    /** taskId → deviation source that paused it */
    const pausedBy = new Map<string, string>();
    for (const src of deviation) {
      const queue = [src];
      const seen = new Set<string>();
      while (queue.length) {
        const cur = queue.pop()!;
        for (const down of dependents.get(cur) ?? []) {
          if (seen.has(down)) continue;
          seen.add(down);
          if (!pausedBy.has(down)) pausedBy.set(down, src);
          queue.push(down);
        }
      }
    }
    const isPaused = (taskId: string): string | null => pausedBy.get(taskId) ?? null;
    // Recount occupancy (D1 global): RUNNING + VERIFYING + REPAIRING
    let global = input.tasks.filter((t) => {
      const s = input.taskStates[t.id];
      return s === "RUNNING" || s === "VERIFYING" || s === "REPAIRING";
    }).length;
    const dispatchable: string[] = [];
    const ineligible: Record<string, DispatchIneligibleReason> = {};
    const ready = input.tasks.filter(
      (t) => states[t.id] === "READY" || promoted.includes(t.id),
    );
    for (const t of ready) {
      const src = isPaused(t.id);
      if (src) {
        ineligible[t.id] = { contractDeviationPaused: { sourceTaskId: src } };
        continue;
      }
      if (global >= maxGlobal) {
        ineligible[t.id] = { globalSlotsFull: { current: global, max: maxGlobal } };
        continue;
      }
      dispatchable.push(t.id);
      global += 1;
    }
    return { promoted, dispatchable, ineligible };
  }
  return invoke<PickDispatchOutput>("pick_dispatchable_tasks", { input });
}

export async function assembleTaskBrief(
  input: TaskBriefInput
): Promise<string> {
  if (!inTauri) {
    return `# Tokage Task Brief v1\n\n## Task\n- ID: ${input.task.id}  Domain: ${input.task.domain}\n- Title: ${input.task.title}\n- Goal: ${input.task.goal}\n- Write scope: ${input.task.write_scope.join(", ")}\n`;
  }
  return invoke<string>("assemble_task_brief_cmd", { input });
}

export async function parseOrGenerateTaskReport(
  task: TaskItem,
  rawOutput: string,
  model: string,
  attemptId: string,
  firstPass: boolean,
  changedFilesStat: string,
  changedFilesList: string[],
  gateResultsSummary: string,
  costUsd?: number | null
): Promise<TaskReport> {
  if (!inTauri) {
    return {
      taskId: task.id,
      title: task.title,
      model,
      attemptId,
      firstPass,
      isLocallyGenerated: !/^#+\s*What was completed/im.test(rawOutput),
      completedWork: rawOutput || "Execution finished",
      tipsForDownstream: REPORT_NONE,
      selfCheckCriteria: [],
      changedFilesStat,
      changedFilesList,
      gateResultsSummary,
      costUsd,
    };
  }
  return invoke<TaskReport>("parse_or_generate_task_report_cmd", {
    task,
    rawOutput,
    model,
    attemptId,
    firstPass,
    changedFilesStat,
    changedFilesList,
    gateResultsSummary,
    costUsd: costUsd ?? null,
  });
}

export async function recordTaskReport(
  report: TaskReportRow
): Promise<number> {
  if (!inTauri) {
    return 1;
  }
  return invoke<number>("record_task_report", { report });
}

/** Register injected tasks too; the database validates the plan's session ownership. */
export async function ensureExecutionTask(sessionId: string, planId: number, task: TaskItem): Promise<number> {
  if (!inTauri) return 1;
  return invoke<number>("ensure_execution_task", { sessionId, planId, task });
}

export async function getTaskReports(
  sessionId: string,
  planRecordId: number,
  taskId: string
): Promise<TaskReportRow[]> {
  if (!inTauri) {
    return [];
  }
  return invoke<TaskReportRow[]>("get_task_reports", { sessionId, planRecordId, taskId });
}

export interface GateCommandResult {
  command: string;
  passed: boolean;
  exitCode?: number | null;
  durationMs: number;
  outputTail: string;
  errorMessage?: string | null;
}

export interface ScopeValidationResult {
  passed: boolean;
  declaredScopes: string[];
  changedFiles: string[];
  allowedFiles: string[];
  outOfScopeFiles: string[];
}

export type RepairStep =
  | "INITIAL"
  | "IN_WINDOW_REPAIR"
  | "REASSIGNED"
  | "BLOCKED";

export interface FailureEvidence {
  failedCommand?: string | null;
  outputTail: string;
  outOfScopeFiles: string[];
  previousChangedFiles: string[];
}

export type RepairDecision =
  | {
      tryInWindowRepair: {
        attemptId: string;
        evidence: string;
      };
    }
  | {
      tryReassign: {
        excludeModels: string[];
        evidence: string;
      };
    }
  | {
      markBlocked: {
        terminalResumeCommand: string;
        reason: string;
      };
    };

export interface GateRunRow {
  sessionId: string;
  planRecordId: number;
  id?: number | null;
  taskId?: string | null;
  planId: string;
  attemptId?: string | null;
  command: string;
  exitCode?: number | null;
  passed: boolean;
  durationMs: number;
  outputTail: string;
  errorMessage?: string | null;
  createdAt?: string | null;
}

export async function executeGateCommand(
  command: string,
  workdir: string,
  timeoutSecs?: number,
  confirmed: boolean = false,
  envAllow?: string[],
  runId?: string
): Promise<GateCommandResult> {
  const demoSession = runId?.match(/tokage-demo-(?:success|repair|error|slow)-(?:en|zh)-[a-z0-9-]+/i)?.[0];
  const config = demoConfig(demoSession, useSettings.getState().demoChunkDelayMs) ?? debugConfig();
  if (!inTauri) {
    if (config.output === "mock") return (await import("./debugMock")).debugMockGate(command, workdir, runId, config);
    return {
      command,
      passed: true,
      exitCode: 0,
      durationMs: 120,
      outputTail: "Mock gate pass",
    };
  }
  return invoke<GateCommandResult>("execute_gate_command_cmd", {
    command,
    workdir,
    timeoutSecs: timeoutSecs ?? null,
    confirmed,
    envAllow: envAllow ?? null,
    runId: runId ?? null,
    debugConfig: config,
  });
}

export async function cancelGateCommand(runId: string): Promise<boolean> {
  if (!inTauri) return false;
  return invoke<boolean>("cancel_gate_command_cmd", { runId });
}

/**
 * F10: pasteable `cd '<cwd>' && <cli> …` resume line for terminal takeover.
 * Returns null when thread id is missing or the CLI cannot resume by id (Aider).
 */
export async function terminalResumeCommand(
  provider: ProviderId,
  threadId: string | null | undefined,
  cwd: string,
): Promise<string | null> {
  if (!inTauri && threadId?.startsWith("tokage-debug:")) return null;
  if (!inTauri) {
    if (!threadId) return null;
    if (provider === "AIDER") return null;
    const argvByProvider: Partial<Record<ProviderId, string[]>> = {
      REASONIX: ["reasonix", "--resume", threadId],
      CLAUDE_CODE: ["claude", "--resume", threadId],
      CODEX_CLI: ["codex", "resume", threadId],
      CURSOR_CLI: ["cursor-agent", "--resume", threadId],
      COPILOT_CLI: ["copilot", "--resume", threadId],
      ANTIGRAVITY_CLI: ["agy", "--resume", threadId],
      OPENCODE: ["opencode", "--session", threadId],
    };
    const argv = argvByProvider[provider];
    if (!argv) return null;
    const escaped = `'${cwd.replace(/'/g, `'\\''`)}'`;
    return `cd ${escaped} && ${argv.join(" ")}`;
  }
  return invoke<string | null>("terminal_resume_command", {
    provider,
    threadId: threadId ?? null,
    cwd,
  });
}

export async function validateWriteScope(
  declaredScopes: string[],
  changedFiles: string[]
): Promise<ScopeValidationResult> {
  if (!inTauri) {
    return {
      passed: true,
      declaredScopes,
      changedFiles,
      allowedFiles: changedFiles,
      outOfScopeFiles: [],
    };
  }
  return invoke<ScopeValidationResult>("validate_write_scope_cmd", {
    declaredScopes,
    changedFiles,
  });
}

export async function revertOutOfScopeFiles(
  workdir: string,
  files: string[],
  turnId: string
): Promise<string> {
  if (!inTauri) throw new Error("Rollback requires native content backups");
  return invoke<string>("revert_out_of_scope_files_cmd", {
    workdir,
    files,
    turnId,
  });
}

/** Local ladder mirror for mock / browser (matches Rust `decide_repair_action`). */
function mockDecideRepairAction(
  currentStep: RepairStep,
  currentAttemptId: string,
  currentModel: string,
  provider: string,
  evidence: FailureEvidence,
  isProviderMelted: boolean,
): [RepairStep, RepairDecision] {
  const evidenceMd = evidence.outputTail || "";
  if (isProviderMelted && (currentStep === "INITIAL" || currentStep === "IN_WINDOW_REPAIR")) {
    return [
      "REASSIGNED",
      { tryReassign: { excludeModels: [currentModel], evidence: evidenceMd } },
    ];
  }
  if (currentStep === "INITIAL") {
    return [
      "IN_WINDOW_REPAIR",
      { tryInWindowRepair: { attemptId: currentAttemptId, evidence: evidenceMd } },
    ];
  }
  if (currentStep === "IN_WINDOW_REPAIR") {
    return [
      "REASSIGNED",
      { tryReassign: { excludeModels: [currentModel], evidence: evidenceMd } },
    ];
  }
  return [
    "BLOCKED",
    {
      markBlocked: {
        terminalResumeCommand: `${provider} --resume ${currentAttemptId}`,
        reason: i18n.t("mock.repair.ladderExhausted"),
      },
    },
  ];
}

export async function decideRepairAction(
  task: TaskItem,
  currentStep: RepairStep,
  currentAttemptId: string,
  currentModel: string,
  provider: string,
  evidence: FailureEvidence,
  isProviderMelted?: boolean,
): Promise<[RepairStep, RepairDecision]> {
  const melted = isProviderMelted ?? false;
  if (!inTauri) {
    return mockDecideRepairAction(
      currentStep,
      currentAttemptId,
      currentModel,
      provider,
      evidence,
      melted,
    );
  }
  return invoke<[RepairStep, RepairDecision]>("decide_repair_action_cmd", {
    task,
    currentStep,
    currentAttemptId,
    currentModel,
    provider,
    evidence,
    isProviderMelted: melted,
  });
}

/** F13.d: reassign one task excluding listed models. */
export async function reassignTask(
  plan: Plan,
  taskId: string,
  excludeModels: string[],
  availableModels?: AvailableModel[],
): Promise<TaskAssignment> {
  if (!inTauri) {
    const task = plan.tasks.find((t) => t.id === taskId);
    if (!task) throw new Error(`task not found: ${taskId}`);
    const candidates = [
      { provider: "CLAUDE_CODE", modelId: "claude-3-7-sonnet", cliModelArg: "sonnet" },
      { provider: "CODEX_CLI", modelId: "gpt-4o", cliModelArg: "gpt-4o" },
      { provider: "CLAUDE_CODE", modelId: "claude-opus", cliModelArg: "opus" },
    ].filter((c) => !excludeModels.includes(c.modelId) && !excludeModels.includes(c.cliModelArg));
    const pick = candidates[0];
    if (!pick) throw new Error("no qualified candidate after exclusions");
    return {
      taskId,
      provider: pick.provider,
      modelId: pick.modelId,
      displayName: pick.modelId,
      cliModelArg: pick.cliModelArg,
      pSuccess: 0.8,
      estCostUsd: 0.1,
      meetsThreshold: true,
      explanation: "mock reassign",
      alternatives: [],
    };
  }
  return invoke<TaskAssignment>("reassign_task", {
    plan,
    taskId,
    excludeModels,
    availableModels: availableModels ?? null,
  });
}

/** F13.f: usage totals by phase for a plan. */
export async function planCostSummary(planId: number): Promise<{
  planning: number;
  execution: number;
  repair: number;
  total: number;
  unknownCostEvents: number;
}> {
  if (!inTauri) {
    return { planning: 0, execution: 0, repair: 0, total: 0, unknownCostEvents: 0 };
  }
  return invoke("plan_cost_summary", { planId });
}

export async function generateImplementationFixTask(
  testTask: TaskItem,
  implementationTask: TaskItem,
  defectDescription: string,
  testFailureEvidence: string
): Promise<TaskItem> {
  if (!inTauri) {
    return {
      ...implementationTask,
      id: `${implementationTask.id}-fix`,
      title: i18n.t("mock.repair.fixTitle", { title: implementationTask.title }),
      depends_on: [testTask.id],
    };
  }
  return invoke<TaskItem>("generate_implementation_fix_task_cmd", {
    testTask,
    implementationTask,
    defectDescription,
    testFailureEvidence,
  });
}

export async function recordGateRun(run: GateRunRow): Promise<number> {
  if (!inTauri) {
    return 1;
  }
  return invoke<number>("record_gate_run", { run });
}

export async function listGateRuns(
  sessionId: string,
  planRecordId: number,
  taskId?: string
): Promise<GateRunRow[]> {
  if (!inTauri) {
    return [];
  }
  return invoke<GateRunRow[]>("list_gate_runs", {
    taskId: taskId ?? null,
    sessionId,
    planRecordId,
  });
}

export interface DeliverySummary {
  planId: string;
  goal: string;
  totalTasks: number;
  completedTasks: number;
  skippedTasks: number;
  allGatesPassed: boolean;
  fullGateResult?: GateCommandResult | null;
  touchedFiles: string[];
  estimatedCostUsd: number;
  actualCostUsd: number;
  costVariancePct: number;
  reviewTask?: TaskItem | null;
  taskModels: Record<string, string>;
}

export interface AttemptRow {
  /** Runtime task code when no legacy numeric task row exists. */
  taskCode?: string | null;
  id: string;
  mainSessionId: string;
  taskId?: number | null;
  role: string;
  providerName: string;
  modelId: string;
  cliWindowIdentity?: string | null;
  reusedFromId?: string | null;
  predecessorId?: string | null;
  execMode: string;
  debugOutput?: DebugRoute["output"] | null;
  requestedProvider?: string | null;
  requestedModel?: string | null;
  workdir: string;
  status: string;
  baselineTokens: number;
  contextTokens: number;
  tokenSource: string;
  turnCount: number;
  lastTurnEndedAt?: string | null;
  touchedFilesJson: string;
  createdAt?: string | null;
  endedAt?: string | null;
}

export type MainSessionStatus =
  | "PLANNING"
  | "REVIEW"
  | "RUNNING"
  | "PAUSED"
  | "BLOCKED"
  | "IDLE"
  | "COMPLETED";

export interface AttemptNode {
  id: string;
  taskId?: string | null;
  role: "PLANNER" | "EXECUTOR" | "TESTER" | "REVIEWER";
  providerName: string;
  modelId: string;
  predecessorId?: string | null;
  reusedFromId?: string | null;
  status: string;
  turnCount: number;
}

export interface TaskNode {
  id: string;
  title: string;
  domain: string;
  state: TaskState;
  assignedModel?: string | null;
  attempts: AttemptNode[];
}

export interface PlanNode {
  planId: string;
  version: number;
  goal: string;
  tasks: TaskNode[];
}

export interface SessionTreeNode {
  sessionId: string;
  projectId: string;
  title: string;
  status: MainSessionStatus;
  plans: PlanNode[];
  mountedExternalSessions: string[];
}

export async function generateDeliverySummary(
  plan: Plan,
  reports: TaskReport[],
  fullGateResult?: GateCommandResult | null,
  estimatedCost: number = 0.0,
  createReviewTask: boolean = false,
  planId?: string | null,
): Promise<DeliverySummary> {
  if (!inTauri) {
    return {
      planId: planId || "plan-mock",
      goal: plan.spec.goal,
      totalTasks: plan.tasks.length,
      completedTasks: reports.length,
      skippedTasks: plan.tasks.length - reports.length,
      allGatesPassed: fullGateResult?.passed === true,
      fullGateResult: fullGateResult ?? null,
      touchedFiles: reports.flatMap((r) => r.changedFilesList),
      estimatedCostUsd: estimatedCost,
      actualCostUsd: reports.reduce((acc, r) => acc + (r.costUsd ?? 0), 0),
      costVariancePct: 0,
      reviewTask: null,
      taskModels: {},
    };
  }
  return invoke<DeliverySummary>("generate_delivery_summary_cmd", {
    plan,
    reports,
    fullGateResult: fullGateResult ?? null,
    estimatedCost,
    createReviewTask,
    planId: planId ?? null,
  });
}

export interface RecordPlanApprovedResult {
  planId: number;
  /** Task code → tasks.id */
  taskIds: Record<string, number>;
}

/** F12.c: write plans/contracts/tasks/edges on approve (idempotent per session+version). */
export async function recordPlanApproved(input: {
  sessionId: string;
  plan: Plan;
  assignments: TaskAssignment[];
  estCostUsd: number;
}): Promise<RecordPlanApprovedResult> {
  if (!inTauri) {
    const taskIds: Record<string, number> = {};
    input.plan.tasks.forEach((t, i) => {
      taskIds[t.id] = i + 1;
    });
    return { planId: 1, taskIds };
  }
  return invoke<RecordPlanApprovedResult>("record_plan_approved", {
    input: {
      sessionId: input.sessionId,
      plan: input.plan,
      assignments: input.assignments,
      estCostUsd: input.estCostUsd,
    },
  });
}

export async function deriveSessionTitleFromSpec(
  spec: PlanSpec
): Promise<string> {
  if (!inTauri) {
    return spec.goal.slice(0, 40) || i18n.t("mock.session.defaultTitle");
  }
  return invoke<string>("derive_session_title_from_spec_cmd", { spec });
}

export async function upsertAttempt(att: AttemptRow): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("upsert_attempt_cmd", { att: debugAttempt(att) });
}

export async function listAttempts(sessionId?: string, projectId?: string): Promise<AttemptRow[]> {
  if (!inTauri) return [];
  return invoke<AttemptRow[]>("list_attempts_cmd", {
    sessionId: sessionId ?? null,
    projectId: projectId ?? null,
  });
}

// F13.b: `update_main_session_status_cmd` removed — session state is TS `setSessionState`.

// ---------------------------------------------------------------------------
// Context Lifecycle Engine (docs/05, P8.m1)
// ---------------------------------------------------------------------------

export type ContextActionKind = "CONTINUE" | "COMPACT" | "FRESH";
export type ActionTrigger = "PLANNER" | "HARD_LIMIT" | "USER";
export type ContextActionStatus = "SUGGESTED" | "APPLIED" | "DISMISSED" | "UNDONE" | "FAILED";
export type CompactMethod = "NATIVE_SLASH_COMMAND" | "INSTRUCTION_PROMPT" | "SYNTHESIZED_FRESH_BRIEF";

/** Read-only context inputs and cost preview; never authorizes an action. */
export function resolveContextInputs(request: import("./contextInputs").ContextInputRequest): Promise<import("./contextInputs").ResolvedContextInputs> {
  return invoke("resolve_context_inputs_cmd", { request });
}

export interface InWindowContextParams {
  baselineTokens: number;
  historyTokens: number;
  remainingTurns: number;
  apiCallsPerTurn: number;
  growthPerTurn: number;
  outputPerTurn: number;
  compactOutputRatio?: number;
  reworkPhi?: number;
  minSaving?: number;
  cacheReadMultiplier?: number;
  cacheWriteMultiplier?: number;
  cacheTtlSeconds?: number;
  elapsedSecondsSinceLastTurn: number;
  cacheType?: string;
  implicitHitReliability?: number;
  effectiveContext?: number;
  contextWindow?: number;
  cliAutoCompactRatio?: number;
  turnsSinceLastCompact?: number;
  pIn?: number;
  pOut?: number;
}

export interface ContextActionPlan {
  cooldownBlocked?: boolean;
  cooldownRemainingTurns?: number;
  recommendedAction: ContextActionKind;
  trigger: ActionTrigger;
  continueCostUsd: number;
  compactCostUsd: number;
  freshCostUsd: number;
  estimatedSavingPercentage: number;
  warmNow: boolean;
  cacheTtlRemainingSeconds: number;
  continueDisqualified: boolean;
  explanation: string;
}

export interface WindowReuseParams {
  modelA: string;
  modelB: string;
  roleA: string;
  roleB: string;
  domainA: string;
  domainB: string;
  writeScopeA: string[];
  writeScopeB: string[];
  historyTokensA: number;
  relevantHistoryTokensA?: number | null;
  baselineTokens: number;
  briefTokensB: number;
  expectedTurnsB: number;
  apiCallsPerTurn: number;
  growthPerTurn: number;
  outputPerTurn: number;
  elapsedSeconds: number;
  cacheTtlSeconds?: number;
  effectiveContext?: number;
  contextWindow?: number;
  cliAutoCompactRatio?: number;
  reworkPhi?: number;
  minSaving?: number;
  pIn?: number;
  pOut?: number;
  cacheReadMultiplier?: number;
  cacheWriteMultiplier?: number;
}

export interface WindowReuseDecision {
  canReuse: boolean;
  reuseCostUsd: number;
  freshCostUsd: number;
  reworkCostUsd: number;
  savingPercentage: number;
  reason: string;
}

export interface TaskSummaryForTriage {
  taskId: string;
  title: string;
  writeScope: string[];
}

export interface TriageMessageParams {
  message: string;
  activeTasks: TaskSummaryForTriage[];
  lastDirectAttemptId?: number | null;
  lastDirectTimestampMs?: number | null;
  currentTimeMs: number;
  explicitTaskId?: string | null;
}

export type AppendMessageRoute =
  | { type: "TASK_EXECUTION_WINDOW"; taskId: string; taskTitle: string; reason: string }
  | { type: "INCREMENTAL_REPLAN"; prompt: string; reason: string }
  | { type: "PREVIOUS_DIRECT_WINDOW"; attemptId: number; reason: string }
  | { type: "DIRECT_DISPATCH"; fresh: boolean; reason: string };

export interface BuildCompactRequest {
  providerName: string;
  taskGoal: string;
  contractPaths: string[];
  pendingErrors?: string | null;
  customInstruction?: string | null;
}

export interface CompactExecutionPayload {
  method: CompactMethod;
  command?: string | null;
  prompt: string;
  instruction: string;
}

export interface ContextActionRow {
  id?: number | null;
  attemptId: string;
  kind: ContextActionKind;
  trigger: ActionTrigger;
  status: ContextActionStatus;
  estCostsJson: string;
  estSavingPct: number;
  actualSavingUsd?: number | null;
  newAttemptId?: string | null;
  createdAt?: string | null;
  appliedAt?: string | null;
}

export async function evaluateWindowReuse(
  params: WindowReuseParams
): Promise<WindowReuseDecision> {
  if (!inTauri) {
    return {
      canReuse: false,
      reuseCostUsd: 0,
      freshCostUsd: 0,
      reworkCostUsd: 0,
      savingPercentage: 0,
      reason: "Mock: window reuse disabled outside Tauri",
    };
  }
  return invoke<WindowReuseDecision>("evaluate_window_reuse_cmd", { params });
}

export interface WindowConfig {
  model: string;
  role: string;
  startupArgs: string[];
}

/** R10: reject briefs whose fixed header drifted (docs/05 §8). */
export async function validateTaskBriefHeader(brief: string): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("validate_task_brief_header_cmd", { brief });
}

/** R9: reject in-window model/role/startup changes (docs/05 §8). */
export async function validateWindowContinuity(
  initial: WindowConfig,
  current: WindowConfig,
): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("validate_window_continuity_cmd", { initial, current });
}

/** R10: reject synthetic prefixes on user messages (docs/05 §8). */
export async function validateUserMessageIntegrity(
  rawUserInput: string,
  transmittedMessage: string,
): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("validate_user_message_integrity_cmd", {
    rawUserInput,
    transmittedMessage,
  });
}

export async function triageMainSessionMessage(
  params: TriageMessageParams
): Promise<AppendMessageRoute> {
  if (!inTauri) {
    return {
      type: "DIRECT_DISPATCH",
      fresh: true,
      reason: "Mock triage route",
    };
  }
  return invoke<AppendMessageRoute>("triage_main_session_message_cmd", { params });
}

export async function listContextActions(
  attemptId: string
): Promise<ContextActionRow[]> {
  if (!inTauri) return [];
  return invoke<ContextActionRow[]>("list_context_actions_cmd", { attemptId });
}

export async function settleContextSavings(
  actionId: number,
  baselineContinueCost: number,
  actualTurnCost: number
): Promise<number> {
  if (!inTauri) return 0.23;
  return invoke<number>("settle_context_savings_cmd", {
    actionId,
    baselineContinueCost,
    actualTurnCost,
  });
}

// ---------------------------------------------------------------------------
// P8.m2: Quota & Circuit Breaker (docs/06 §4, §6, docs/09 R12)
// ---------------------------------------------------------------------------

/** Mirrors Rust `ProviderHealthRow` (camelCase serde). */
export interface ProviderHealthRow {
  providerName: string;
  position: number;
  status: string;
  cliVersion: string | null;
  infoJson: string;
  meltReason: string | null;
  meltedUntil: string | null;
  consecutiveFails: number;
  updatedAt: string | null;
}

export type ProviderMeltedEvent = {
  provider: string;
  reason: string;
  until: string | null;
};

export type ProviderRecoveredEvent = {
  provider: string;
  reason: string | null;
  until: string | null;
};

export interface ProviderUtilizationProjection {
  provider: string;
  utilizationPct: number | null;
  source: string;
  quotaEquivCostUsd: number | null;
}

export interface PlanBudgetCheckResult {
  estCostUsd: number;
  actualCostUsd: number;
  repairCostUsd?: number;
  overrunRatio: number;
  is50PctOverrun: boolean;
  isRepairOverrun?: boolean;
  isRedlineExceeded: boolean;
  shouldPauseQueue: boolean;
  message: string;
}

export async function recoverProviderHealth(
  providerName: string,
  newStatus?: string
): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("recover_provider_health_cmd", {
    providerName,
    newStatus: newStatus ?? null,
  });
}

export async function listProviderHealth(): Promise<ProviderHealthRow[]> {
  if (!inTauri) return [];
  return invoke<ProviderHealthRow[]>("list_provider_health_cmd");
}

const mockMeltListeners = new Set<(e: ProviderMeltedEvent) => void>();
const mockRecoverListeners = new Set<(e: ProviderRecoveredEvent) => void>();

/** F15.e: circuit breaker tripped (`provider-melted`). */
export async function onProviderMelted(
  cb: (e: ProviderMeltedEvent) => void,
): Promise<() => void> {
  if (!inTauri) {
    mockMeltListeners.add(cb);
    return () => {
      mockMeltListeners.delete(cb);
    };
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ProviderMeltedEvent>("provider-melted", (e) => cb(e.payload));
}

/** F15.e: circuit breaker recovered (`provider-recovered`). */
export async function onProviderRecovered(
  cb: (e: ProviderRecoveredEvent) => void,
): Promise<() => void> {
  if (!inTauri) {
    mockRecoverListeners.add(cb);
    return () => {
      mockRecoverListeners.delete(cb);
    };
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ProviderRecoveredEvent>("provider-recovered", (e) => cb(e.payload));
}

/** Test/browser helper for melt events. */
export function emitMockProviderMelted(event: ProviderMeltedEvent): void {
  mockMeltListeners.forEach((fn) => fn(event));
}

/** Test/browser helper for recover events. */
export function emitMockProviderRecovered(event: ProviderRecoveredEvent): void {
  mockRecoverListeners.forEach((fn) => fn(event));
}

export interface OnlineUsagePermission { allowOnlineUsageQuery: boolean; generation: number }
export async function getOnlineUsagePermission(): Promise<OnlineUsagePermission> {
  if (!inTauri) return { allowOnlineUsageQuery: false, generation: 0 };
  return invoke<OnlineUsagePermission>("get_online_usage_permission");
}
export async function setOnlineUsagePermission(allowed: boolean): Promise<OnlineUsagePermission> {
  if (!inTauri) throw new Error("Online usage permission requires the native application");
  return invoke<OnlineUsagePermission>("set_online_usage_permission", { allowed });
}
export async function onOnlineUsagePermissionChanged(handler: (state: OnlineUsagePermission) => void): Promise<() => void> {
  if (!inTauri) return Promise.resolve(() => {});
  const { listen } = await import("@tauri-apps/api/event");
  return listen<OnlineUsagePermission>("online-usage-permission-changed", (event) => handler(event.payload));
}

/** Explicit quota refresh; native installation-local consent is required. */
export async function queryProviderQuota(provider: ProviderId, automatic = false): Promise<AccountUsageResult> {
  if (!inTauri) return { status: "DISABLED", identityKey: null, identityGeneration: null, latestSuccess: null, fromCache: false, attemptedAt: Date.now() / 1000, nextRetryAt: null, error: null };
  return invoke<AccountUsageResult>("query_provider_quota", { provider, automatic });
}
export interface UsageRevisions { all: number; providers: Partial<Record<ProviderId, number>> }
export async function onUsageBindingInvalidated(handler: (revisions: UsageRevisions) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<UsageRevisions>("usage-binding-invalidated", (event) => handler(event.payload));
}
export async function getProviderUsageState(provider: ProviderId): Promise<AccountUsageResult | null> {
  if (!inTauri) return null;
  return invoke<AccountUsageResult>("get_provider_usage_state", { provider });
}

export async function getProviderUtilizationNow(
  provider: ProviderId,
  equivCostSinceLast: number = 0.0
): Promise<ProviderUtilizationProjection> {
  if (!inTauri) {
    return {
      provider,
      utilizationPct: 15.0,
      source: "Estimated",
      quotaEquivCostUsd: 0.15,
    };
  }
  return invoke<ProviderUtilizationProjection>("get_provider_utilization_now_cmd", {
    provider,
    equivCostSinceLast,
  });
}

export async function reassignUnstartedTasks(
  plan: Plan,
  currentAssignments: TaskAssignment[],
  completedOrRunningTaskIds: string[],
  meltedProviders: string[],
  availableModels?: AvailableModel[],
): Promise<PlanAllocationResult> {
  if (!inTauri) {
    return allocatePlanModels(plan);
  }
  return invoke<PlanAllocationResult>("reassign_unstarted_tasks_cmd", {
    plan,
    currentAssignments,
    completedOrRunningTaskIds,
    meltedProviders,
    availableModels: availableModels ?? null,
  });
}

export async function checkPlanBudgetOverrun(
  estCostUsd: number,
  currentActualCostUsd: number,
  planBudgetUsd?: number,
  repairCostUsd?: number,
  pauseOnOverrun?: boolean,
): Promise<PlanBudgetCheckResult> {
  const pauseSoft = pauseOnOverrun ?? true;
  const repair = repairCostUsd ?? 0;
  if (!inTauri) {
    const is50PctOverrun = estCostUsd > 0 && currentActualCostUsd > estCostUsd * 1.5;
    const isRepairOverrun = estCostUsd > 0 && repair > estCostUsd * 0.3;
    const isRedlineExceeded = (planBudgetUsd ?? 0) > 0 && currentActualCostUsd >= (planBudgetUsd ?? 0);
    return {
      estCostUsd,
      actualCostUsd: currentActualCostUsd,
      repairCostUsd: repair,
      overrunRatio: estCostUsd > 0 ? (currentActualCostUsd - estCostUsd) / estCostUsd : 0,
      is50PctOverrun,
      isRepairOverrun,
      isRedlineExceeded,
      shouldPauseQueue: isRedlineExceeded || (pauseSoft && (is50PctOverrun || isRepairOverrun)),
      message: isRedlineExceeded
        ? i18n.t("mock.budget.exceeded")
        : isRepairOverrun
          ? i18n.t("mock.budget.repair30")
          : is50PctOverrun
            ? i18n.t("mock.budget.over50")
            : i18n.t("mock.budget.ok"),
    };
  }
  return invoke<PlanBudgetCheckResult>("check_plan_budget_overrun_cmd", {
    estCostUsd,
    currentActualCostUsd,
    planBudgetUsd: planBudgetUsd ?? null,
    repairCostUsd: repairCostUsd ?? null,
    pauseOnOverrun: pauseOnOverrun ?? null,
  });
}

// ---------------------------------------------------------------------------
// P8.m3: Usage & Quality Dashboard and Savings Ledger (docs/06 §7, P8.m3a/b)
// ---------------------------------------------------------------------------

export type MeltReason =
  | "QUOTA_SOFT"
  | "QUOTA_HARD"
  | "BUDGET"
  | "HEALTH"
  | "AUTH";

export interface DashboardModelUsage {
  modelId: string;
  inputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  outputTokens: number;
  costUsd: number;
  equivCostUsd: number;
  calls: number;
}

export interface DailyModelUsagePoint {
  modelId: string;
  provider: string;
  totalTokens: number;
  inputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  outputTokens: number;
  costUsd: number;
  equivCostUsd: number;
}

export interface DailyUsagePoint {
  date: string;
  totalTokens: number;
  inputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  outputTokens: number;
  costUsd: number;
  equivCostUsd: number;
  models?: DailyModelUsagePoint[];
}

export interface DashboardProviderInfo {
  provider: ProviderId;
  billing: "SUBSCRIPTION" | "PAY_AS_YOU_GO" | "UNKNOWN";
  isGateway?: boolean;
  customEndpoint?: string | null;
  usagePct?: number;
  usagePctSource?: "SNAPSHOT" | "ESTIMATED" | "CLI_OUTPUT";
  resetAt?: string;
  spentUsd?: number;
  equivCostUsd?: number;
  cacheHitRate: number;
  status: ProviderStatus;
  meltReason?: MeltReason;
  models?: DashboardModelUsage[];
}

export interface QualityMatrixEntry {
  model: string;
  domain: string;
  tasks: number;
  firstPassRate: number;
}

export interface DashboardQuality {
  firstPassRate: number;
  matrix: QualityMatrixEntry[];
}

export interface DashboardSavings {
  totalUsd: number;
  byKind: Record<"ASSIGNMENT" | "CONTEXT" | "DIRECT", number>;
  referenceModel: string;
}

export interface DashboardCostSplit {
  planningUsd: number;
  executionUsd: number;
  repairUsd: number;
}

export interface DashboardTokenSeriesPoint {
  bucket: string;
  provider: ProviderId;
  input: number;
  cacheRead: number;
  cacheWrite: number;
  output: number;
}

export interface UsageDashboard {
  range: "today" | "7d" | "30d";
  providers: DashboardProviderInfo[];
  quality: DashboardQuality;
  savings: DashboardSavings;
  costSplit: DashboardCostSplit;
  tokenSeries: DashboardTokenSeriesPoint[];
  dailyUsage: DailyUsagePoint[];
}

export interface SavingsEventRow {
  id?: number;
  mainSessionId?: string | null;
  planId?: number | null;
  kind: "ASSIGNMENT" | "CONTEXT" | "DIRECT";
  refId?: string | null;
  baselineUsd: number;
  actualUsd?: number | null;
  settled: boolean;
  detailJson: string;
  createdAt?: string | null;
}

export async function getUsageDashboard(
  range: "today" | "7d" | "30d" = "7d"
): Promise<UsageDashboard> {
  if (!inTauri) {
    const today = new Date();
    const daysCount = 365;
    const dailyUsageMock: DailyUsagePoint[] = Array.from({ length: daysCount }, (_, i) => {
      const d = new Date(today);
      d.setDate(d.getDate() - (daysCount - 1 - i));
      const dateStr = d.toISOString().slice(0, 10);
      const isWeekend = d.getDay() === 0 || d.getDay() === 6;
      const hasActivity = !isWeekend || i % 6 === 0;
      if (!hasActivity) {
        return {
          date: dateStr,
          totalTokens: 0,
          inputTokens: 0,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          outputTokens: 0,
          costUsd: 0,
          equivCostUsd: 0,
        };
      }
      const factor = isWeekend ? 0.3 : (0.4 + ((i * 7) % 11) / 10);
      const input = Math.round(35000 * factor);
      const cacheRead = Math.round(160000 * factor);
      const cacheWrite = Math.round(11000 * factor);
      const output = Math.round(14000 * factor);
      const totalTokens = input + cacheRead + cacheWrite + output;

      const m1Input = Math.round(input * 0.75);
      const m1Read = Math.round(cacheRead * 0.75);
      const m1Write = Math.round(cacheWrite * 0.75);
      const m1Output = Math.round(output * 0.75);
      const m1Total = m1Input + m1Read + m1Write + m1Output;
      const m1Cost = Number((0.17 * factor).toFixed(3));
      const m1Equiv = Number((0.32 * factor).toFixed(3));

      const m2Input = input - m1Input;
      const m2Read = cacheRead - m1Read;
      const m2Write = cacheWrite - m1Write;
      const m2Output = output - m1Output;
      const m2Total = m2Input + m2Read + m2Write + m2Output;
      const m2Cost = Number((0.05 * factor).toFixed(3));
      const m2Equiv = Number((0.10 * factor).toFixed(3));

      return {
        date: dateStr,
        totalTokens,
        inputTokens: input,
        cacheReadTokens: cacheRead,
        cacheWriteTokens: cacheWrite,
        outputTokens: output,
        costUsd: Number((0.22 * factor).toFixed(3)),
        equivCostUsd: Number((0.42 * factor).toFixed(3)),
        models: [
          {
            modelId: "claude-3-7-sonnet",
            provider: "CLAUDE_CODE",
            totalTokens: m1Total,
            inputTokens: m1Input,
            cacheReadTokens: m1Read,
            cacheWriteTokens: m1Write,
            outputTokens: m1Output,
            costUsd: m1Cost,
            equivCostUsd: m1Equiv,
          },
          {
            modelId: "claude-3-5-haiku",
            provider: "CLAUDE_CODE",
            totalTokens: m2Total,
            inputTokens: m2Input,
            cacheReadTokens: m2Read,
            cacheWriteTokens: m2Write,
            outputTokens: m2Output,
            costUsd: m2Cost,
            equivCostUsd: m2Equiv,
          },
        ],
      };
    });

    return {
      range,
      providers: [
        {
          provider: "CLAUDE_CODE",
          billing: "UNKNOWN",
          isGateway: true,
          customEndpoint: "https://api.openai-proxy.internal/v1",
          usagePct: 0.42,
          usagePctSource: "SNAPSHOT",
          cacheHitRate: 88.5,
          status: "READY",
          spentUsd: 1.25,
          equivCostUsd: 2.10,
          models: [
            {
              modelId: "claude-3-7-sonnet",
              inputTokens: 185000,
              cacheReadTokens: 1240000,
              cacheWriteTokens: 95000,
              outputTokens: 72000,
              costUsd: 1.05,
              equivCostUsd: 1.75,
              calls: 42,
            },
            {
              modelId: "claude-3-5-haiku",
              inputTokens: 65000,
              cacheReadTokens: 420000,
              cacheWriteTokens: 30000,
              outputTokens: 28000,
              costUsd: 0.20,
              equivCostUsd: 0.35,
              calls: 18,
            },
          ],
        },
        {
          provider: "CODEX_CLI",
          billing: "SUBSCRIPTION",
          usagePct: 0.18,
          usagePctSource: "SNAPSHOT",
          cacheHitRate: 91.0,
          status: "READY",
          spentUsd: 0.45,
          equivCostUsd: 0.85,
          models: [
            {
              modelId: "gpt-4o",
              inputTokens: 92000,
              cacheReadTokens: 710000,
              cacheWriteTokens: 45000,
              outputTokens: 38000,
              costUsd: 0.45,
              equivCostUsd: 0.85,
              calls: 24,
            },
          ],
        },
        {
          provider: "CURSOR_CLI",
          billing: "SUBSCRIPTION",
          usagePct: 0.25,
          usagePctSource: "ESTIMATED",
          cacheHitRate: 75.0,
          status: "READY",
          models: [
            {
              modelId: "claude-3-5-sonnet",
              inputTokens: 60000,
              cacheReadTokens: 450000,
              cacheWriteTokens: 35000,
              outputTokens: 25000,
              costUsd: 0.35,
              equivCostUsd: 0.60,
              calls: 15,
            },
          ],
        },
        {
          provider: "AIDER",
          billing: "PAY_AS_YOU_GO",
          cacheHitRate: 0.0,
          status: "READY",
          spentUsd: 0.31,
          models: [
            {
              modelId: "deepseek/deepseek-chat",
              inputTokens: 120000,
              cacheReadTokens: 0,
              cacheWriteTokens: 0,
              outputTokens: 45000,
              costUsd: 0.31,
              equivCostUsd: 0.31,
              calls: 9,
            },
          ],
        },
      ],
      quality: {
        firstPassRate: 0.91,
        matrix: [
          { model: "claude-3-7-sonnet", domain: "architecture", tasks: 8, firstPassRate: 0.95 },
          { model: "claude-3-7-sonnet", domain: "backend-api", tasks: 12, firstPassRate: 0.92 },
          { model: "gpt-4o", domain: "frontend-ui", tasks: 10, firstPassRate: 0.88 },
          { model: "claude-3-5-haiku", domain: "testing", tasks: 15, firstPassRate: 0.93 },
        ],
      },
      savings: {
        totalUsd: 12.4,
        byKind: {
          ASSIGNMENT: 8.5,
          CONTEXT: 2.7,
          DIRECT: 1.2,
        },
        referenceModel: "claude-3-7-sonnet",
      },
      costSplit: {
        planningUsd: 0.85,
        executionUsd: 4.20,
        repairUsd: 0.35,
      },
      tokenSeries: [
        { bucket: "10-01", provider: "CLAUDE_CODE", input: 120000, cacheRead: 850000, cacheWrite: 65000, output: 45000 },
        { bucket: "10-02", provider: "CLAUDE_CODE", input: 95000, cacheRead: 720000, cacheWrite: 50000, output: 38000 },
        { bucket: "10-03", provider: "CODEX_CLI", input: 80000, cacheRead: 610000, cacheWrite: 40000, output: 32000 },
        { bucket: "10-04", provider: "CLAUDE_CODE", input: 140000, cacheRead: 980000, cacheWrite: 75000, output: 52000 },
        { bucket: "10-05", provider: "CURSOR_CLI", input: 60000, cacheRead: 450000, cacheWrite: 35000, output: 25000 },
      ],
      dailyUsage: dailyUsageMock,
    };
  }
  return invoke<UsageDashboard>("get_usage_dashboard", { range });
}

export async function settlePlanSavings(
  planId: number,
  userOverride?: string
): Promise<number | null> {
  if (debugConfig().output === "mock") return null;
  if (!inTauri) return 1;
  return invoke<number | null>("settle_plan_savings_cmd", {
    planId,
    userOverride: userOverride ?? null,
  });
}

export async function recordSavingsEvent(params: {
  sessionId?: string;
  planId?: number;
  kind: "ASSIGNMENT" | "CONTEXT" | "DIRECT";
  refId?: string;
  baselineUsd: number;
  actualUsd?: number;
  detailJson?: string;
}): Promise<number> {
  const source = params.sessionId ? (await import("./debugProvider")).debugSessionConfig(params.sessionId)?.output ?? debugConfig().output : debugConfig().output;
  if (source !== "off") return 0;
  if (!inTauri) return 1;
  return invoke<number>("record_savings_event_cmd", {
    sessionId: params.sessionId ?? null,
    planId: params.planId ?? null,
    kind: params.kind,
    refId: params.refId ?? null,
    baselineUsd: params.baselineUsd,
    actualUsd: params.actualUsd ?? null,
    detailJson: params.detailJson ?? null,
  });
}

// ---------------------------------------------------------------------------------------
// P8.m5 Data Retention & FTS Logs IPC

export interface ChatLogRow {
  id: number;
  messageId: string;
  mainSessionId: string;
  attemptId: string;
  seq: number;
  turnIndex: number;
  role: string;
  content: string;
  contentTruncated: boolean;
  metaJson: string;
  createdAt: string;
}

export interface ProjectExportData {
  project: {
    id: string;
    name: string;
    localPath: string;
    position: number;
    expanded: boolean;
    trusted?: boolean;
    createdAt?: string;
  };
  sessions: Array<{
    id: string;
    title: string;
    status: string;
    position: number;
    archived: boolean;
    forkedFrom?: string | null;
    worktree?: any;
    createdAt: string;
    updatedAt: string;
  }>;
  chatLogs: Array<{
    id: number;
    messageId: string;
    mainSessionId: string;
    seq: number;
    turnIndex: number;
    role: string;
    content: string;
    contentTruncated: boolean;
    meta: any;
    createdAt: string;
  }>;
  exportedAt: string;
}

export async function getChatLogs(
  attemptId: string,
  afterId?: number,
  limit?: number
): Promise<ChatLogRow[]> {
  if (!inTauri) return [];
  return invoke<ChatLogRow[]>("get_chat_logs_cmd", {
    attemptId,
    afterId: afterId ?? null,
    limit: limit ?? null,
  });
}

export async function searchChatLogs(
  query: string,
  sessionId?: string,
  limit?: number
): Promise<ChatLogRow[]> {
  if (!inTauri) return [];
  return invoke<ChatLogRow[]>("search_chat_logs_cmd", {
    query,
    sessionId: sessionId ?? null,
    limit: limit ?? null,
  });
}

export async function cleanupRetentionRecords(): Promise<[number, number]> {
  if (!inTauri) return [0, 0];
  return invoke<[number, number]>("cleanup_retention_records_cmd");
}

export async function exportProjectData(projectId: string): Promise<ProjectExportData> {
  if (!inTauri) {
    return {
      project: { id: projectId, name: "Mock Project", localPath: "/mock", position: 0, expanded: true },
      sessions: [],
      chatLogs: [],
      exportedAt: new Date().toISOString(),
    };
  }
  return invoke<ProjectExportData>("export_project_data_cmd", { projectId });
}

export async function clearProjectRecords(projectId: string): Promise<[number, number]> {
  if (!inTauri) return [0, 0];
  return invoke<[number, number]>("clear_project_records_cmd", { projectId });
}









