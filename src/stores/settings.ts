import { create } from "zustand";
import type { AuthMode, ProviderId } from "../lib/ipc";

export type ThemePreference = "system" | "light" | "dark";
export type RequestMode = "auto" | "plan" | "direct";
export type ContextMode = "auto" | "suggest" | "off";
export type IsolationMode = "worktree" | "inplace";

/** Bumped when defaults that must migrate existing installs change (S10 / F1.g / F11.i). */
export const SETTINGS_VERSION = 3;

export interface SettingsState {
  theme: ThemePreference;
  language: "en" | "zh";

  // Planning & models (docs/02, docs/04)
  requestMode: RequestMode;
  /** Start the composer in the mode last used instead of `requestMode`. */
  rememberMode: boolean;
  lastMode: RequestMode;
  plannerModel: string; // "auto" or a model id
  referenceModel: string;
  confidenceThreshold: number; // 0..1
  requireReview: boolean;
  /** Automatically approve plan when estimated cost is below this USD value (0 = disabled). docs/02 §8, P5.m3c */
  planningAutoApproveBelowUsd: number;

  // Context (docs/05)
  contextMode: ContextMode;
  cacheAware: boolean;
  contextMinSaving: number; // 0..1, default 0.15 (15%)

  // Usage & budget (docs/06)
  planBudgetUsd: number;
  pauseOnOverrun: boolean;
  meltThreshold: number; // 0..1
  activeQuotaQuery: boolean;

  // Execution (docs/03, docs/09)
  timeoutMultiplier: number;
  /** Sub-agent idle timeout in minutes (D5 / F3). Absolute; not multiplied. */
  agentIdleTimeoutMin: number;
  /** Sub-agent hard timeout in minutes (D5 / F3). Absolute; ≥ agentIdleTimeoutMin. */
  agentHardTimeoutMin: number;
  isolation: IsolationMode;
  /** Persisted schema version for one-shot default migrations (S10). */
  settingsVersion: number;
  maxParallel: number;
  /** New conversations start in their own git worktree (composer checkbox, remembered). */
  sessionWorktree: boolean;
  /**
   * Env var names kept in gate subprocesses after credential scrubbing (F5.d).
   * Exact names or `PREFIX_*` patterns; default empty.
   */
  gateEnvAllow: string[];

  // CLI management (docs/01)
  providerEnabled: Partial<Record<ProviderId, boolean>>;
  providerPaths: Partial<Record<ProviderId, string>>;
  /** Non-featured whitelist entries the user added to CLI management. */
  providerAdded: ProviderId[];
  /**
   * Whether Tokage may pass workspace trust to each CLI when running it in a project the
   * user added. Unset = not asked yet. Only `true` makes the CLI's models selectable.
   */
  providerTrust: Partial<Record<ProviderId, boolean>>;

  /**
   * Per project id: whether `.tokage/` goes into the project's `.gitignore`. Asked on the
   * first write to `.tokage/` (P1.m6e); unset = not asked yet.
   */
  tokageGitignore: Record<string, boolean>;

  // Permissions (docs/11 §2). `full_auto` is never stored: it is granted per project for a
  // limited time and ends with the app.
  /** Global default mode. */
  authMode: Exclude<AuthMode, "full_auto">;
  /** Per project id: overrides `authMode`. Unset = inherit. */
  authProjectModes: Record<string, Exclude<AuthMode, "full_auto">>;

  // Knowledge pack (docs/04)
  knowledgeAutoUpdate: boolean;

  /** The first-launch notice (docs/11 §7.1) was acknowledged. */
  welcomeSeen: boolean;

  // Dev shadow model & Model blocking
  /** Force all actual requests to run on a surrogate model in dev while keeping UI display intact. */
  devShadowModel: "none" | "luna" | "deepseek";
  /** Models completely blocked from selection and recommendation (matched by key or substring). */
  blockedModels: string[];
  /** Model banning level: off = none, moderate = ban Fable, Astra; strict = additionally ban Opus, Sol, etc. */
  modelBanningLevel: "off" | "moderate" | "strict";
  /**
   * Per-model provider preferences and soft-banning:
   * e.g. { "opus": { prefer: "CLAUDE_CODE", never: ["CURSOR_CLI", "COPILOT_CLI"] } }
   */
  modelProviderPreferences: Record<string, { prefer?: string; never?: string[] }>;

  set: <K extends keyof SettingsData>(key: K, value: SettingsData[K]) => void;
  setProviderEnabled: (id: ProviderId, enabled: boolean) => void;
  setProviderPath: (id: ProviderId, path: string) => void;
  addProvider: (id: ProviderId) => void;
  removeProvider: (id: ProviderId) => void;
  setProviderTrust: (id: ProviderId, trusted: boolean) => void;
  toggleBlockedModel: (modelKey: string) => void;
  removeBlockedModel: (modelKey: string) => void;
  setModelBanningLevel: (level: "off" | "moderate" | "strict") => void;
  setModelProviderPreference: (
    modelKey: string,
    pref: { prefer?: string; never?: string[] },
  ) => void;
}

export type SettingsData = Omit<
  SettingsState,
  | "set"
  | "setProviderEnabled"
  | "setProviderPath"
  | "addProvider"
  | "removeProvider"
  | "setProviderTrust"
  | "toggleBlockedModel"
  | "removeBlockedModel"
  | "setModelBanningLevel"
  | "setModelProviderPreference"
>;

/**
 * One-shot migration for installs that persisted old defaults via db.capture() (S10).
 * - Version < 2: knowledgeAutoUpdate → false (F1.g / F8).
 * - Version < 3: isolation → worktree (F11.i; design default after full worktree path).
 */
export function migrateSettings(values: Partial<SettingsData>): Partial<SettingsData> {
  const version = typeof values.settingsVersion === "number" ? values.settingsVersion : 0;
  if (version >= SETTINGS_VERSION) return values;

  const next: Partial<SettingsData> = { ...values, settingsVersion: SETTINGS_VERSION };
  if (version < 2) {
    next.knowledgeAutoUpdate = false;
  }
  if (version < 3) {
    next.isolation = "worktree";
  }
  return next;
}

export const useSettings = create<SettingsState>()(
  (set) => ({
    theme: "system",
    language: "en",
    requestMode: "auto",
    rememberMode: false,
    lastMode: "auto",
    plannerModel: "auto",
    referenceModel: "auto",
    confidenceThreshold: 0.8,
    requireReview: true,
    planningAutoApproveBelowUsd: 0,
    contextMode: "suggest",
    cacheAware: true,
    contextMinSaving: 0.15,
    planBudgetUsd: 0,
    pauseOnOverrun: true,
    meltThreshold: 0.9,
    activeQuotaQuery: false,
    timeoutMultiplier: 1,
    agentIdleTimeoutMin: 30,
    agentHardTimeoutMin: 120,
    isolation: "worktree",
    settingsVersion: SETTINGS_VERSION,
    maxParallel: 3,
    sessionWorktree: false,
    gateEnvAllow: [],
    providerEnabled: {},
    providerPaths: {},
    providerAdded: [],
    providerTrust: {},
    tokageGitignore: {},
    authMode: "ask_sensitive",
    authProjectModes: {},
    knowledgeAutoUpdate: false,
    welcomeSeen: false,
    devShadowModel: "none",
    blockedModels: [],
    modelBanningLevel: "off",
    modelProviderPreferences: {
      opus: { prefer: "CLAUDE_CODE", never: ["CURSOR_CLI", "COPILOT_CLI"] },
    },

    set: (key, value) =>
      set((s) => {
        const next: Partial<SettingsState> = { [key]: value } as Partial<SettingsState>;
        // D5: keep hard ≥ idle when either agent timeout setting changes.
        if (key === "agentIdleTimeoutMin") {
          const idle = value as number;
          if (s.agentHardTimeoutMin < idle) next.agentHardTimeoutMin = idle;
        }
        if (key === "agentHardTimeoutMin") {
          const hard = value as number;
          if (hard < s.agentIdleTimeoutMin) next.agentHardTimeoutMin = s.agentIdleTimeoutMin;
        }
        return next;
      }),

    setProviderEnabled: (id, enabled) =>
      set((s) => ({ providerEnabled: { ...s.providerEnabled, [id]: enabled } })),
    setProviderPath: (id, path) =>
      set((s) => ({ providerPaths: { ...s.providerPaths, [id]: path } })),
    addProvider: (id) =>
      set((s) => (s.providerAdded.includes(id) ? s : { providerAdded: [...s.providerAdded, id] })),
    removeProvider: (id) => set((s) => ({ providerAdded: s.providerAdded.filter((x) => x !== id) })),
    setProviderTrust: (id, trusted) =>
      set((s) => ({ providerTrust: { ...s.providerTrust, [id]: trusted } })),
    toggleBlockedModel: (modelKey) =>
      set((s) => {
        const norm = modelKey.trim().toLowerCase();
        if (!norm) return s;
        const exists = s.blockedModels.some((b) => b.toLowerCase() === norm);
        return {
          blockedModels: exists
            ? s.blockedModels.filter((b) => b.toLowerCase() !== norm)
            : [...s.blockedModels, modelKey.trim()],
        };
      }),
    removeBlockedModel: (modelKey) =>
      set((s) => ({
        blockedModels: s.blockedModels.filter((b) => b.toLowerCase() !== modelKey.trim().toLowerCase()),
      })),
    setModelBanningLevel: (level) => set({ modelBanningLevel: level }),
    setModelProviderPreference: (modelKey, pref) =>
      set((s) => {
        const key = modelKey.trim().toLowerCase();
        const existing = s.modelProviderPreferences[key] || {};
        return {
          modelProviderPreferences: {
            ...s.modelProviderPreferences,
            [key]: {
              ...existing,
              ...pref,
            },
          },
        };
      }),
  }),
);
