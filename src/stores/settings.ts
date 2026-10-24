import { create } from "zustand";
import { DEFAULT_DEBUG_PROVIDER, type DebugProviderConfig } from "../lib/debugProviderTypes";
import type { AuthMode, ProviderId } from "../lib/ipc";
import {
  addCustomFamilyFromCatalog,
  banUltraFamilies,
  defaultModelChannelAccess,
  familyOf,
  migrateToModelChannelAccess,
  removeCustomFamily,
  setFamilyChannel,
  type ModelChannelAccess,
} from "../lib/modelAccess";

export type ThemePreference = "system" | "light" | "dark";
export type RequestMode = "auto" | "plan" | "direct" | "fixed";
export type ContextMode = "auto" | "suggest" | "off";
export type IsolationMode = "worktree" | "inplace";

/** Bumped when defaults that must migrate existing installs change (S10 / F1.g / F11.i / model matrix). */
export const SETTINGS_VERSION = 6;

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
  /** Append `Co-authored-by: Tokage <345094078+tokage-app[bot]@users.noreply.github.com>` when committing or merging (default false). */
  tokageCoAuthor: boolean;
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

  /** Unified debug execution provider. Selected models remain logical routing identities. */
  debugProvider: DebugProviderConfig;
  demoEnabled: boolean;
  demoScenario: import("../lib/debugProviderTypes").MockScenario;
  demoChunkDelayMs: number;
  /** @deprecated Read only for migration from the old shadow-model setting. */
  devShadowModel?: "none" | "luna" | "deepseek";
  /**
   * Model-family × channel allow matrix (source of truth for bans + prefer).
   * channels[provider]=false denies; denyAll denies every channel; prefer soft-ranks recommend.
   */
  modelChannelAccess: ModelChannelAccess;
  /**
   * @deprecated Migrated into modelChannelAccess (v4). Kept for read fallback / CliSettings chips.
   */
  blockedModels: string[];
  /** @deprecated Migrated into modelChannelAccess denyAll (v4). */
  modelBanningLevel: "off" | "moderate" | "strict";
  /**
   * @deprecated prefer/never migrated into modelChannelAccess (v4). prefer still mirrored for older callers.
   */
  modelProviderPreferences: Record<string, { prefer?: string; never?: string[] }>;
  /**
   * Model ids the user typed in for a CLI whose catalog doesn't list them (e.g. a full Claude
   * model name). Offered in every picker next to the probed models.
   */
  customModels: Partial<Record<ProviderId, string[]>>;
  /**
   * `PROVIDER:requested` → the model the CLI reported actually running (init/result events).
   * Learns what an alias like `opus` resolves to without any extra request.
   */
  observedModels: Record<string, string>;

  set: <K extends keyof SettingsData>(key: K, value: SettingsData[K]) => void;
  setProviderEnabled: (id: ProviderId, enabled: boolean) => void;
  setProviderPath: (id: ProviderId, path: string) => void;
  addProvider: (id: ProviderId) => void;
  removeProvider: (id: ProviderId) => void;
  setProviderTrust: (id: ProviderId, trusted: boolean) => void;
  /** Toggle denyAll for the model family (CLI chip / legacy). */
  toggleBlockedModel: (modelKey: string) => void;
  removeBlockedModel: (modelKey: string) => void;
  setModelBanningLevel: (level: "off" | "moderate" | "strict") => void;
  setModelProviderPreference: (
    modelKey: string,
    pref: { prefer?: string; never?: string[] },
  ) => void;
  setModelChannelAccess: (access: ModelChannelAccess) => void;
  addCustomModel: (id: ProviderId, model: string) => void;
  removeCustomModel: (id: ProviderId, model: string) => void;
  recordObservedModel: (provider: ProviderId, requested: string, actual: string) => void;
  setFamilyChannelAllowed: (
    family: string,
    providerId: string,
    allowed: boolean,
    otherProviderIds?: string[],
  ) => void;
  setFamilyPrefer: (family: string, prefer: string | undefined) => void;
  banUltraExpensiveFamilies: () => void;
  /** Add a matrix row from a probed catalog model. Returns the family key (builtin or custom). */
  addModelFamilyFromCatalog: (modelKey: string, modelName: string) => string | null;
  removeCustomModelFamily: (familyKey: string) => void;
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
  | "setModelChannelAccess"
  | "setFamilyChannelAllowed"
  | "setFamilyPrefer"
  | "banUltraExpensiveFamilies"
  | "addModelFamilyFromCatalog"
  | "removeCustomModelFamily"
  | "addCustomModel"
  | "removeCustomModel"
  | "recordObservedModel"
>;

/**
 * One-shot migration for installs that persisted old defaults via db.capture() (S10).
 * - Version < 2: knowledgeAutoUpdate → false (F1.g / F8).
 * - Version < 3: isolation → worktree (F11.i; design default after full worktree path).
 * - Version < 4: fold blockedModels / banningLevel / prefs into modelChannelAccess.
 * - Version < 5: contextMode → off (in-window suggestion bar removed; old default was suggest).
 */
export function migrateSettings(values: Partial<SettingsData>): Partial<SettingsData> {
  const clean = { ...values } as Partial<SettingsData> & { activeQuotaQuery?: unknown; allowOnlineUsageQuery?: unknown };
  delete clean.activeQuotaQuery;
  delete clean.allowOnlineUsageQuery;
  values = clean;
  const version = typeof values.settingsVersion === "number" ? values.settingsVersion : 0;
  if (version >= SETTINGS_VERSION) return values;

  const next: Partial<SettingsData> = { ...values, settingsVersion: SETTINGS_VERSION };
  if (version < 2) {
    next.knowledgeAutoUpdate = false;
  }
  if (version < 3) {
    next.isolation = "worktree";
  }
  if (version < 4) {
    next.modelChannelAccess = migrateToModelChannelAccess({
      blockedModels: values.blockedModels,
      modelBanningLevel: values.modelBanningLevel,
      modelProviderPreferences: values.modelProviderPreferences,
      modelChannelAccess: values.modelChannelAccess,
    });
    // Clear legacy hard-ban fields; prefer mirrored from matrix for older recommend callers
    next.blockedModels = [];
    next.modelBanningLevel = "off";
    next.modelProviderPreferences = Object.fromEntries(
      Object.entries(next.modelChannelAccess).map(([k, e]) => [k, { prefer: e.prefer, never: [] as string[] }]),
    );
  }
  if (version < 5) {
    next.contextMode = "off";
  }
  if (version < 6) {
    next.debugProvider = { ...DEFAULT_DEBUG_PROVIDER, ...values.debugProvider, output: values.debugProvider?.output ?? (values.devShadowModel === "luna" || values.devShadowModel === "deepseek" ? values.devShadowModel : "off") };
    delete next.devShadowModel;
  }
  return next;
}

/** Keep deprecated modelProviderPreferences.prefer in sync with the matrix. */
function mirrorPrefsFromAccess(access: ModelChannelAccess): Record<string, { prefer?: string; never?: string[] }> {
  return Object.fromEntries(
    Object.entries(access).map(([k, e]) => [k, { prefer: e.prefer, never: [] as string[] }]),
  );
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
    contextMode: "off",
    cacheAware: true,
    contextMinSaving: 0.15,
    planBudgetUsd: 0,
    pauseOnOverrun: true,
    meltThreshold: 0.95,
    timeoutMultiplier: 1,
    agentIdleTimeoutMin: 30,
    agentHardTimeoutMin: 120,
    isolation: "worktree",
    settingsVersion: SETTINGS_VERSION,
    maxParallel: 3,
    sessionWorktree: false,
    tokageCoAuthor: false,
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
    debugProvider: { ...DEFAULT_DEBUG_PROVIDER },
    demoEnabled: false,
    demoScenario: "success",
    demoChunkDelayMs: 30,
    modelChannelAccess: defaultModelChannelAccess(),
    blockedModels: [],
    modelBanningLevel: "off",
    modelProviderPreferences: {
      opus: { prefer: "CLAUDE_CODE", never: [] },
    },
    customModels: {},
    observedModels: {},

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
        if (key === "modelChannelAccess") {
          next.modelProviderPreferences = mirrorPrefsFromAccess(value as ModelChannelAccess);
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
        const family = familyOf(modelKey) ?? modelKey.trim().toLowerCase();
        if (!family) return s;
        const access = structuredClone(s.modelChannelAccess);
        const entry = access[family] ?? { channels: {} };
        const currentlyDenied = !!entry.denyAll;
        entry.denyAll = !currentlyDenied;
        if (!currentlyDenied) {
          // denying: clear per-channel so denyAll is the sole signal
          entry.channels = {};
        }
        access[family] = entry;
        return {
          modelChannelAccess: access,
          modelProviderPreferences: mirrorPrefsFromAccess(access),
          // Keep legacy list roughly in sync for any leftover readers
          blockedModels: currentlyDenied
            ? s.blockedModels.filter((b) => familyOf(b) !== family && b.toLowerCase() !== family)
            : [...s.blockedModels.filter((b) => familyOf(b) !== family && b.toLowerCase() !== family), family],
        };
      }),
    removeBlockedModel: (modelKey) =>
      set((s) => {
        const family = familyOf(modelKey) ?? modelKey.trim().toLowerCase();
        const access = structuredClone(s.modelChannelAccess);
        if (access[family]) {
          access[family].denyAll = false;
        }
        return {
          modelChannelAccess: access,
          modelProviderPreferences: mirrorPrefsFromAccess(access),
          blockedModels: s.blockedModels.filter((b) => b.toLowerCase() !== modelKey.trim().toLowerCase()),
        };
      }),
    setModelBanningLevel: (level) =>
      set((s) => {
        // Legacy API: map onto denyAll for ultra families
        let access = structuredClone(s.modelChannelAccess);
        const moderate = ["fable", "astra"];
        const strict = ["fable", "astra", "opus", "sol"];
        for (const f of strict) {
          if (!access[f]) access[f] = { channels: {} };
          access[f].denyAll = false;
        }
        if (level === "moderate") {
          for (const f of moderate) {
            if (!access[f]) access[f] = { channels: {} };
            access[f].denyAll = true;
          }
        } else if (level === "strict") {
          access = banUltraFamilies(access);
        }
        return {
          modelBanningLevel: level,
          modelChannelAccess: access,
          modelProviderPreferences: mirrorPrefsFromAccess(access),
        };
      }),
    setModelProviderPreference: (modelKey, pref) =>
      set((s) => {
        const key = (familyOf(modelKey) ?? modelKey.trim().toLowerCase()) as string;
        const access = structuredClone(s.modelChannelAccess);
        const entry = access[key] ?? { channels: {} };
        if ("prefer" in pref) entry.prefer = pref.prefer;
        if (pref.never) {
          for (const id of pref.never) entry.channels[id] = false;
        }
        access[key] = entry;
        return {
          modelChannelAccess: access,
          modelProviderPreferences: mirrorPrefsFromAccess(access),
        };
      }),
    addCustomModel: (id, model) =>
      set((s) => {
        const m = model.trim();
        const list = s.customModels[id] ?? [];
        if (!m || list.includes(m)) return s;
        return { customModels: { ...s.customModels, [id]: [...list, m] } };
      }),
    removeCustomModel: (id, model) =>
      set((s) => ({
        customModels: { ...s.customModels, [id]: (s.customModels[id] ?? []).filter((x) => x !== model) },
      })),
    recordObservedModel: (provider, requested, actual) =>
      set((s) => {
        const key = `${provider}:${requested}`;
        return s.observedModels[key] === actual
          ? s
          : { observedModels: { ...s.observedModels, [key]: actual } };
      }),
    setModelChannelAccess: (access) =>
      set({
        modelChannelAccess: access,
        modelProviderPreferences: mirrorPrefsFromAccess(access),
      }),
    setFamilyChannelAllowed: (family, providerId, allowed, otherProviderIds = []) =>
      set((s) => {
        const access = setFamilyChannel(
          s.modelChannelAccess,
          family,
          providerId,
          allowed,
          otherProviderIds,
        );
        return {
          modelChannelAccess: access,
          modelProviderPreferences: mirrorPrefsFromAccess(access),
        };
      }),
    setFamilyPrefer: (family, prefer) =>
      set((s) => {
        const access = structuredClone(s.modelChannelAccess);
        const entry = access[family] ?? { channels: {} };
        if (prefer) entry.prefer = prefer;
        else delete entry.prefer;
        access[family] = entry;
        return {
          modelChannelAccess: access,
          modelProviderPreferences: mirrorPrefsFromAccess(access),
        };
      }),
    banUltraExpensiveFamilies: () =>
      set((s) => {
        const access = banUltraFamilies(s.modelChannelAccess);
        return {
          modelChannelAccess: access,
          modelProviderPreferences: mirrorPrefsFromAccess(access),
        };
      }),
    addModelFamilyFromCatalog: (modelKey, modelName) => {
      let resultKey: string | null = null;
      set((s) => {
        const result = addCustomFamilyFromCatalog(s.modelChannelAccess, modelKey, modelName);
        if (!result) return s;
        resultKey = result.key;
        if (result.alreadyPresent) return s;
        return {
          modelChannelAccess: result.access,
          modelProviderPreferences: mirrorPrefsFromAccess(result.access),
        };
      });
      return resultKey;
    },
    removeCustomModelFamily: (familyKey) =>
      set((s) => {
        const access = removeCustomFamily(s.modelChannelAccess, familyKey);
        return {
          modelChannelAccess: access,
          modelProviderPreferences: mirrorPrefsFromAccess(access),
        };
      }),
  }),
);
