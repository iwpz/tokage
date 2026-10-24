import { create } from "zustand";
import {
  getProviderUtilizationNow,
  getProviderUsageState,
  onUsageBindingInvalidated,
  type AccountUsageResult,
  getDebugProviderCatalog,
  listProviderHealth,
  onProviderDetected,
  onProviderMelted,
  onProviderRecovered,
  queryProviderQuota,
  recoverProviderHealth,
  refreshProvider,
  verifyAntigravityApiKey,
  verifyReasonixApiKey,
  refreshProviders,
  type ProviderHealthRow,
  type ProviderId,
  type ProviderInfo,
  type ProviderMeltedEvent,
  type ProviderRecoveredEvent,
} from "../lib/ipc";
import { useOnlineUsage } from "./onlineUsage";
import { useSettings } from "./settings";
import { builtinDebugCatalog, debugCatalogOverlay } from "../lib/debugProvider";

interface ProvidersState {
  /** Last known results; restored from cache on startup, then replaced row by row. */
  providers: ProviderInfo[];
  /** Actual installation/authentication state, unaffected by debug catalogs. */
  realProviders: ProviderInfo[];
  debugCatalog: ProviderInfo[];
  /** provider_health rows keyed by provider name (F15.e). */
  healthByProvider: Record<string, ProviderHealthRow>;
  scanning: boolean;
  /** CLIs whose fresh result hasn't arrived yet in the current scan. */
  pending: ProviderId[];
  lastScan: number | null;
  usageRevision: number;
  usageResults: Partial<Record<ProviderId, AccountUsageResult>>;
  error: string | null;
  /** Startup: show the cached list immediately, then rescan in the background. */
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  /** Re-detects one CLI only (its card's refresh button); the rest stay as they are. */
  refreshOne: (id: ProviderId) => Promise<void>;
  /** Merge a health row from list / melt / recover events. */
  upsertHealth: (row: ProviderHealthRow) => void;
  applyMelted: (e: ProviderMeltedEvent) => void;
  applyRecovered: (e: ProviderRecoveredEvent) => void;
  /** Manual recover from melt badge (F15.e). */
  recover: (providerName: string) => Promise<void>;
  /** Refresh quota for one CLI (settings page). */
  refreshQuota: (id: ProviderId, automatic?: boolean) => Promise<void>;
  verifyAntigravityKey: () => Promise<void>;
  verifyReasonixKey: () => Promise<void>;
  /** True when provider is MELTED (not selectable). HALF_OPEN remains runnable for probe. */
  isMelted: (providerName: string) => boolean;
  healthOf: (providerName: string) => ProviderHealthRow | undefined;
}

let unlisten: Promise<() => void> | null = null;
let usageUnlisten: Promise<() => void> | null = null;
let healthUnlisten: Promise<() => void> | null = null;
let currentScan: string | null = null;
export const useProviders = create<ProvidersState>()((set, get) => ({
  providers: [],
  realProviders: [],
  debugCatalog: builtinDebugCatalog(),
  healthByProvider: {},
  scanning: false,
  pending: [],
  lastScan: null,
  usageResults: {},
  usageRevision: 0,
  error: null,

  isMelted: (providerName) => get().healthByProvider[providerName]?.status === "MELTED",

  healthOf: (providerName) => get().healthByProvider[providerName],

  upsertHealth: (row) =>
    set((s) => ({
      healthByProvider: { ...s.healthByProvider, [row.providerName]: row },
    })),

  applyMelted: (e) =>
    set((s) => {
      const prev = s.healthByProvider[e.provider];
      const row: ProviderHealthRow = {
        providerName: e.provider,
        position: prev?.position ?? 0,
        status: "MELTED",
        cliVersion: prev?.cliVersion ?? null,
        infoJson: prev?.infoJson ?? "{}",
        meltReason: e.reason,
        meltedUntil: e.until,
        consecutiveFails: prev?.consecutiveFails ?? 0,
        updatedAt: new Date().toISOString(),
      };
      return { healthByProvider: { ...s.healthByProvider, [e.provider]: row } };
    }),

  applyRecovered: (e) =>
    set((s) => {
      const prev = s.healthByProvider[e.provider];
      const row: ProviderHealthRow = {
        providerName: e.provider,
        position: prev?.position ?? 0,
        status: "READY",
        cliVersion: prev?.cliVersion ?? null,
        infoJson: prev?.infoJson ?? "{}",
        meltReason: null,
        meltedUntil: null,
        consecutiveFails: 0,
        updatedAt: new Date().toISOString(),
      };
      return { healthByProvider: { ...s.healthByProvider, [e.provider]: row } };
    }),

  recover: async (providerName) => {
    await recoverProviderHealth(providerName, "READY");
    get().applyRecovered({ provider: providerName, reason: null, until: null });
  },

  refreshQuota: async (id, automatic = false) => {
    const selectedSettings = useSettings.getState();
    const sourcePath = selectedSettings.providerPaths[id];
    const usageRevision = get().usageRevision;
    if (selectedSettings.providerEnabled[id] === false) return;
    await useOnlineUsage.getState().load();
    const consent = useOnlineUsage.getState();
    if (!consent.ready || !consent.allowOnlineUsageQuery || consent.busy) {
      set({ error: "Online usage query is disabled" });
      return;
    }
    try {
      const result = await queryProviderQuota(id, automatic);
      const current = useOnlineUsage.getState();
      if (get().usageRevision === usageRevision && current.allowOnlineUsageQuery && !current.busy && current.generation === consent.generation && useSettings.getState().providerEnabled[id] !== false && useSettings.getState().providerPaths[id] === sourcePath) set((s) => {
        const previous = s.usageResults[id];
        if (previous?.identityGeneration != null && result.identityGeneration != null && (
          previous.identityGeneration > result.identityGeneration || (previous.identityGeneration === result.identityGeneration && previous.attemptedAt > result.attemptedAt)
        )) return {};
        const snapshot = result.latestSuccess;
        const identityChanged = result.status === "IDENTITY_CHANGED" || result.status === "IDENTITY_UNKNOWN";
        const realProviders = s.realProviders.map((p) => {
          if (p.id !== id || !p.account || p.usageQuery?.backend) return p;
          const quantities = snapshot?.quantities.filter((q) => q.unit === "currency" && q.remaining != null) ?? [];
          return { ...p, account: { ...p.account,
            usage: identityChanged ? null : snapshot?.display ?? p.account.usage,
            balance: identityChanged ? null : quantities.length > 0 ? quantities.map((q) => `${q.currency} ${q.remaining}`).join(" · ") : p.account.balance,
          } };
        });
        return { usageResults: { ...s.usageResults, [id]: result }, realProviders,
          providers: debugCatalogOverlay(realProviders, s.debugCatalog, useSettings.getState().debugProvider),
          error: result.error ? `Usage query: ${result.error}` : null };
      });
    } catch (e) {
      set({ error: String(e) });
    }
    try {
      await getProviderUtilizationNow(id, 0);
    } catch {
      /* optional */
    }
  },

  verifyAntigravityKey: async () => {
    const id = "ANTIGRAVITY_CLI" as const;
    if (get().scanning || get().pending.includes(id)) return;
    set((s) => ({ error: null, pending: [...s.pending, id] }));
    try {
      const info = await verifyAntigravityApiKey();
      set((s) => {
        const realProviders = s.realProviders.some((p) => p.id === id)
          ? s.realProviders.map((p) => p.id === id ? info : p) : [...s.realProviders, info];
        return { realProviders, providers: debugCatalogOverlay(realProviders, s.debugCatalog, useSettings.getState().debugProvider) };
      });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set((s) => ({ pending: s.pending.filter((p) => p !== id) }));
    }
  },

  verifyReasonixKey: async () => {
    const id = "REASONIX" as const;
    if (get().scanning || get().pending.includes(id)) return;
    set((s) => ({ error: null, pending: [...s.pending, id] }));
    try {
      const info = await verifyReasonixApiKey();
      set((s) => {
        const realProviders = s.realProviders.some((p) => p.id === id)
          ? s.realProviders.map((p) => p.id === id ? info : p) : [...s.realProviders, info];
        return { realProviders, providers: debugCatalogOverlay(realProviders, s.debugCatalog, useSettings.getState().debugProvider) };
      });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set((s) => ({ pending: s.pending.filter((p) => p !== id) }));
    }
  },

  load: async () => {
    await useOnlineUsage.getState().load();
    usageUnlisten ??= onUsageBindingInvalidated(() => set((s) => ({ usageRevision: s.usageRevision + 1, usageResults: {} })));
    await usageUnlisten;
    // Wire melt/recover listeners once.
    healthUnlisten ??= (async () => {
      const offMelt = await onProviderMelted((e) => {
        get().applyMelted(e);
        void import("../lib/taskRunner").then((m) => m.onProviderMeltedEvent(e));
      });
      const offRec = await onProviderRecovered((e) => {
        get().applyRecovered(e);
        void import("../lib/taskRunner").then((m) => m.onProviderRecoveredEvent(e));
      });
      return () => {
        offMelt();
        offRec();
      };
    })();
    await healthUnlisten;

    try {
      const rows = await listProviderHealth();
      const map: Record<string, ProviderHealthRow> = {};
      for (const r of rows) map[r.providerName] = r;
      set({ healthByProvider: map });
    } catch {
      /* empty until first melt */
    }

    try {
      const debugCatalog = await getDebugProviderCatalog();
      set({ debugCatalog, providers: debugCatalogOverlay(get().realProviders, debugCatalog, useSettings.getState().debugProvider) });
    } catch { /* bundled catalog remains available offline */ }
    await get().refresh();
    const summaries = await Promise.all(get().realProviders.map(async (p) => [p.id, await getProviderUsageState(p.id).catch(() => null)] as const));
    set((s) => ({ usageResults: { ...s.usageResults, ...Object.fromEntries(summaries.filter(([id, r]) => r != null && (r.identityGeneration ?? 0) >= (s.usageResults[id]?.identityGeneration ?? 0))) } }));
  },

  refresh: async () => {
    if (get().scanning) return;
    const scanId = crypto.randomUUID();
    currentScan = scanId;
    unlisten ??= onProviderDetected(({ scanId: id, provider }) => {
      if (id !== currentScan) return;
      set((s) => {
        const i = s.realProviders.findIndex((p) => p.id === provider.id);
        const realProviders =
          i === -1 ? [...s.realProviders, provider] : s.realProviders.map((p, j) => (j === i ? provider : p));
        return { realProviders, providers: debugCatalogOverlay(realProviders, s.debugCatalog, useSettings.getState().debugProvider), pending: s.pending.filter((p) => p !== provider.id) };
      });
    });
    await unlisten;

    set((s) => ({ scanning: true, error: null, usageResults: {}, usageRevision: s.usageRevision + 1, pending: s.providers.map((p) => p.id) }));
    try {
      // The final list is authoritative: registry order, and drops CLIs removed from the whitelist.
      const providers = await refreshProviders(scanId, useSettings.getState().providerPaths);
      if (currentScan === scanId) set({ realProviders: providers, providers: debugCatalogOverlay(providers, get().debugCatalog, useSettings.getState().debugProvider), lastScan: Date.now() });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      if (currentScan === scanId) set({ scanning: false, pending: [] });
    }
  },

  refreshOne: async (id) => {
    if (get().scanning || get().pending.includes(id)) return;
    set((s) => ({ error: null, usageResults: { ...s.usageResults, [id]: undefined }, usageRevision: s.usageRevision + 1, pending: [...s.pending, id] }));
    try {
      const info = await refreshProvider(id, useSettings.getState().providerPaths[id]);
      set((s) => { const realProviders = s.realProviders.map((p) => p.id === id ? info : p); return { realProviders, providers: debugCatalogOverlay(realProviders, s.debugCatalog, useSettings.getState().debugProvider) }; });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set((s) => ({ pending: s.pending.filter((p) => p !== id) }));
    }
  },
}));

// Debug overlays do not authorize online queries.
useSettings.subscribe((s, prev) => {
  if (s.debugProvider !== prev.debugProvider) {
    const store = useProviders.getState();
    useProviders.setState({ providers: debugCatalogOverlay(store.realProviders, store.debugCatalog, s.debugProvider) });
  }
});
