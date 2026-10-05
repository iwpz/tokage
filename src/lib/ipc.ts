import { invoke, isTauri } from "@tauri-apps/api/core";

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
  | "TRAE_CLI";

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
  /** When the CLI recorded the numbers (ISO 8601). */
  observedAt: string | null;
  source: UsageSourceType;
}

/** Billing channel, read locally. Quotas and list prices only apply when `official`. */
export interface Account {
  billing: Billing;
  official: boolean;
  /** Custom endpoint without userinfo / query / fragment, token-like path segments masked; or a cloud name. */
  endpoint: string | null;
  plan: string | null;
  usage: UsageSnapshot | null;
}

/** Where `models` came from: the CLI's own catalog, or aliases it documents. */
export type ModelsSource = "CLI" | "BUILTIN";

export type GuidanceCode =
  | "INSTALL"
  | "LOGIN"
  | "SET_API_KEY"
  | "VERIFY_LOGIN"
  | "THIRD_PARTY_BACKEND"
  | "NO_MODELS"
  | "TIMEOUT"
  | "UNSUPPORTED_VERSION";

export interface Guidance {
  code: GuidanceCode;
  /** Copyable command; never executed by Tokage. */
  command: string | null;
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
  installMethods: InstallMethod[];
  /** Flags Tokage adds to pass workspace trust (`<workspace>` = project path); empty if none. */
  trustArgs: string[];
  /** Tokage can run conversation turns on this CLI (it has a headless adapter). */
  chat: boolean;
  /** The adapter's capability bits and tested versions; null when `chat` is false. */
  adapter: AdapterInfo | null;
  /** How Direct mode is restricted on this CLI until approvals exist (docs/11 §4.2). */
  directLimit: DirectLimit | null;
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
  | "OWN_SANDBOX";

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
  CODEX_CLI: { caps: [...BASE_CAPS, "OWN_SANDBOX"], versions: ">= 0.160.0, < 1", tested: "0.160.0" },
  CURSOR_CLI: { caps: [...BASE_CAPS, "DENIAL_EVENT"], versions: ">= 2026.10.01", tested: "2026.10.01" },
  COPILOT_CLI: { caps: [...BASE_CAPS, "TOOL_RULES"], versions: ">= 1.0.91, < 2", tested: "1.0.91" },
  ANTIGRAVITY_CLI: { caps: [...BASE_CAPS, "TOOL_RULES", "DENIAL_EVENT", "OS_SANDBOX", "SHELL_WRAP"], versions: ">= 1.0.0, < 3", tested: "1.14.0" },
  AIDER: { caps: ["MODEL_SELECT", "READ_ONLY_MODE", "RESUME", "OS_SANDBOX"], versions: ">= 0.50.0, < 2", tested: "0.72.0" },
  OPENCODE: { caps: [...BASE_CAPS, "TOOL_RULES", "DENIAL_EVENT", "OS_SANDBOX", "SHELL_WRAP"], versions: ">= 0.1.0, < 2", tested: "0.8.4" },
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
    account: { billing: "PAY_AS_YOU_GO", official: false, endpoint: "https://relay.example.org/v1/***", plan: null, usage: null },
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
    account: { billing: "SUBSCRIPTION", official: true, endpoint: null, plan: null, usage: null },
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

export interface Worktree {
  /** Worktree checkout root. */
  root: string;
  /** Where turns run: the project's own folder inside the worktree. */
  cwd: string;
  branch: string;
}

/** New linked worktree on branch `tokage/<name>`, created from HEAD. */
export async function createSessionWorktree(path: string, name: string): Promise<Worktree> {
  if (!inTauri) {
    const root = `~/Library/Application Support/dev.tokage.app/worktrees/mock/${name}`;
    return { root, cwd: root, branch: `tokage/${name}` };
  }
  return invoke<Worktree>("create_session_worktree", { path, name });
}

/**
 * Removes a session worktree and its `tokage/` branch. Rejects with "DIRTY" when the
 * checkout has uncommitted changes and `force` is false.
 */
export async function removeSessionWorktree(root: string, branch: string, force: boolean): Promise<void> {
  if (!inTauri) return;
  await invoke("remove_session_worktree", { root, branch, force });
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
    const text = MOCK_FILES[path.split("/").pop() ?? ""] ?? null;
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
}

export const REPO_URL = "https://github.com/iwpz/tokage";

export async function getBuildInfo(): Promise<BuildInfo> {
  if (!inTauri) {
    return { version: __APP_VERSION__, commit: null, kind: "local", repoUrl: REPO_URL, sourceUrl: `${REPO_URL}/tree/master` };
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

/** Retrieves the built-in knowledge pack JSON bundled with the app (offline available, P6.m1b). */
export async function getBuiltinKnowledgePack(): Promise<string> {
  if (!inTauri) {
    const raw = await import("../../src-tauri/resources/knowledge/builtin.json?raw");
    return raw.default;
  }
  return invoke<string>("get_builtin_knowledge_pack");
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

export async function ignoreTokageDir(path: string): Promise<void> {
  if (inTauri) await invoke("ignore_tokage_dir", { path });
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
  /** Timeout tier (docs/09 §2.1, R2). Defaults based on mode if omitted. */
  timeoutTier?: "QUICK" | "STANDARD" | "LONG" | "PLANNING" | "GATE" | null;
  /** Timeout multiplier from settings (e.g. 1.0, 2.0). */
  timeoutMultiplier?: number | null;
}

export type ToolKind = "command" | "read" | "search" | "edit" | "web" | "agent" | "other";
export type FileChangeKind = "added" | "modified" | "deleted";

export type ChatEventKind =
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
      source?: "STRUCTURED" | "STDOUT_REGEX" | "ESTIMATED";
    }
  | { kind: "error"; message: string }
  | { kind: "warning"; message: string }
  | { kind: "quota"; raw: string; resetAt?: string | null; retrying: boolean };

export type ChatEvent = { turnId: string } & ChatEventKind;

const mockTimers = new Map<string, number>();
let mockListener: ((e: ChatEvent) => void) | null = null;

export async function onChatEvent(cb: (e: ChatEvent) => void): Promise<() => void> {
  if (!inTauri) {
    mockListener = cb;
    return () => {
      mockListener = null;
    };
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ChatEvent>("chat-event", (e) => cb(e.payload));
}

export async function startChatTurn(request: ChatRequest): Promise<void> {
  if (!inTauri) {
    // Browser preview: replay a scripted turn so the UI can be exercised without a backend.
    const steps = mockTurn(request);
    let i = 0;
    const emit = (k: ChatEventKind) => mockListener?.({ turnId: request.turnId, ...k });
    const timer = window.setInterval(() => {
      if (i < steps.length) emit(steps[i++]);
      else {
        window.clearInterval(timer);
        mockTimers.delete(request.turnId);
      }
    }, 60);
    mockTimers.set(request.turnId, timer);
    return;
  }
  await invoke("start_chat_turn", { request });
}

const MOCK_FILES: Record<string, string> = {
  "m.py": "def add(a, b):\n    return a + b\n",
  "test_m.py": "from m import add\n\n\ndef test_add():\n    assert add(2, 3) == 5\n",
};

function mockTurn(r: ChatRequest): ChatEventKind[] {
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
    window.clearInterval(mockTimers.get(turnId));
    mockTimers.delete(turnId);
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

export async function classifyCommand(request: ClassifyRequest): Promise<CommandVerdict> {
  if (!inTauri) throw new Error("Command classification needs the desktop app");
  return invoke<CommandVerdict>("classify_command", { request });
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
  costUsd?: number | null;
  equivCostUsd?: number | null;
  phase: "PLANNING" | "EXECUTION" | "REPAIR" | "CONTEXT" | "DIRECT";
  source: "STRUCTURED" | "STDOUT_REGEX" | "ESTIMATED";
  createdAt?: string | null;
}

export async function listUsageEvents(sessionId?: string | null, planId?: number | null): Promise<UsageEventRow[]> {
  if (!inTauri) return [];
  return invoke<UsageEventRow[]>("list_usage_events", { sessionId, planId });
}

export async function recordUsageEvent(event: UsageEventRow): Promise<number> {
  if (!inTauri) return 0;
  return invoke<number>("record_usage_event", { event });
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

export async function getProviderProxyStatus(provider: ProviderId): Promise<ProviderProxyStatus> {
  if (!inTauri) {
    return {
      provider,
      env_vars: ["HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"],
      extra_env: {},
      own_config_proxy: null,
      supports_socks: false,
      forced_supported: true,
      verified: true,
    };
  }
  return invoke<ProviderProxyStatus>("get_provider_proxy_status", { provider });
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
    return `你是技术规划者...\n# 需求\n${args.userRequest}`;
  }
  return invoke<string>("render_planning_prompt", {
    userRequest: args.userRequest,
    repoDigest: args.repoDigest,
    domains: args.domains ?? null,
    outputMaxTokens: args.outputMaxTokens ?? null,
    customFormatSpec: args.customFormatSpec ?? null,
  });
}

export async function parsePlanningOutput(args: {
  markdown: string;
  defaultTestCommands?: string[];
  defaultDomain?: string;
}): Promise<Plan> {
  if (!inTauri) {
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
  message: string;
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
    return `增量重规划：${args.changeDescription}`;
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
  initialProviderUtils?: Record<string, number>
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
      explanation: `${t.domain} · 难度 ${t.difficulty} · 规模 ${t.size}。Claude Sonnet 5.5 一次通过把握 0.88，预估 $0.22。选择 Claude Sonnet 5.5。`,
      alternatives: [],
    }));
    return {
      assignments,
      totalEstCostUsd: assignments.reduce((acc, a) => acc + a.estCostUsd, 0),
      allMeetThreshold: true,
      providerUtilizations: { CLAUDE_CODE: 0.1 },
    };
  }
  return invoke<PlanAllocationResult>("allocate_plan_models", {
    plan,
    manualOverrides: manualOverrides ?? null,
    initialProviderUtils: initialProviderUtils ?? null,
  });
}

export async function assignDirectTask(
  domain: string,
  difficulty: number,
  size?: TaskSize,
  initialProviderUtils?: Record<string, number>
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
      explanation: `${domain} · 难度 ${difficulty} · 规模 ${size ?? "S"}。Claude Haiku 4.5 一次通过把握 0.90，预估 $0.05。选择 Claude Haiku 4.5。`,
      alternatives: [],
    };
  }
  return invoke<TaskAssignment | null>("assign_direct_task", {
    domain,
    difficulty,
    size: size ?? null,
    initialProviderUtils: initialProviderUtils ?? null,
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
  outcome: TaskOutcomeRow
): Promise<number> {
  if (!inTauri) {
    return 1;
  }
  return invoke<number>("record_task_outcome", { outcome });
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



