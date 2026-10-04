import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { ProviderId } from "../lib/ipc";

export type ThemePreference = "system" | "light" | "dark";
export type RequestMode = "auto" | "plan" | "direct";
export type ContextMode = "auto" | "suggest" | "off";
export type IsolationMode = "worktree" | "inplace";

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

  // Context (docs/05)
  contextMode: ContextMode;
  cacheAware: boolean;

  // Usage & budget (docs/06)
  planBudgetUsd: number;
  pauseOnOverrun: boolean;
  meltThreshold: number; // 0..1
  activeQuotaQuery: boolean;

  // Execution (docs/03, docs/09)
  timeoutMultiplier: number;
  isolation: IsolationMode;
  maxParallel: number;
  /** New conversations start in their own git worktree (composer checkbox, remembered). */
  sessionWorktree: boolean;

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

  // Knowledge pack (docs/04)
  knowledgeAutoUpdate: boolean;

  /** The first-launch notice (docs/11 §7.1) was acknowledged. */
  welcomeSeen: boolean;

  set: <K extends keyof SettingsData>(key: K, value: SettingsData[K]) => void;
  setProviderEnabled: (id: ProviderId, enabled: boolean) => void;
  setProviderPath: (id: ProviderId, path: string) => void;
  addProvider: (id: ProviderId) => void;
  removeProvider: (id: ProviderId) => void;
  setProviderTrust: (id: ProviderId, trusted: boolean) => void;
}

export type SettingsData = Omit<
  SettingsState,
  "set" | "setProviderEnabled" | "setProviderPath" | "addProvider" | "removeProvider" | "setProviderTrust"
>;

export const useSettings = create<SettingsState>()(
  persist(
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
      contextMode: "suggest",
      cacheAware: true,
      planBudgetUsd: 0,
      pauseOnOverrun: true,
      meltThreshold: 0.9,
      activeQuotaQuery: false,
      timeoutMultiplier: 1,
      isolation: "worktree",
      maxParallel: 3,
      sessionWorktree: false,
      providerEnabled: {},
      providerPaths: {},
      providerAdded: [],
      providerTrust: {},
      knowledgeAutoUpdate: true,
      welcomeSeen: false,

      set: (key, value) => set({ [key]: value } as Partial<SettingsState>),
      setProviderEnabled: (id, enabled) =>
        set((s) => ({ providerEnabled: { ...s.providerEnabled, [id]: enabled } })),
      setProviderPath: (id, path) =>
        set((s) => ({ providerPaths: { ...s.providerPaths, [id]: path } })),
      addProvider: (id) =>
        set((s) => (s.providerAdded.includes(id) ? s : { providerAdded: [...s.providerAdded, id] })),
      removeProvider: (id) => set((s) => ({ providerAdded: s.providerAdded.filter((x) => x !== id) })),
      setProviderTrust: (id, trusted) =>
        set((s) => ({ providerTrust: { ...s.providerTrust, [id]: trusted } })),
    }),
    { name: "tokage.settings", version: 1 },
  ),
);
