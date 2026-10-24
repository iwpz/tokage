import { create } from "zustand";
import {
  getProviderUtilizationNow,
  getProviderUsageState,
  onUsageBindingInvalidated,
  type AccountUsageResult,
  type UsageRevisions,
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
import { createUsageQueue } from "../lib/usageQueue";
import { canQueryUsage } from "../lib/usagePolicy";
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
  initialized: boolean;
  initializing: boolean;
  usageVersions: Partial<Record<ProviderId, number>>;
  usageRevisions: UsageRevisions;
  providerErrors: Partial<Record<ProviderId, string>>;
  usageErrors: Partial<Record<ProviderId, string>>;
  usageResults: Partial<Record<ProviderId, AccountUsageResult>>;
  usagePending: ProviderId[];
  usageActive: ProviderId[];
  error: string | null;
  /** Startup: show the cached list immediately, then rescan in the background. */
  load: () => Promise<void>;
  syncUsage: (ids: ProviderId[]) => Promise<void>;
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
const usageQueue = createUsageQueue();
let loadFlight: Promise<void> | null = null;
let scanFlight: Promise<void> | null = null;
const revisionOf = (v: UsageRevisions, id: ProviderId) => Math.max(v.all, v.providers[id] ?? 0);
/** State restoration never overwrites a newer observation in the same identity generation. */
function acceptUsage(previous: AccountUsageResult | undefined, next: AccountUsageResult): boolean {
  if (!previous) return true;
  const a = previous.identityGeneration ?? 0, b = next.identityGeneration ?? 0;
  return b > a || (b === a && next.attemptedAt >= previous.attemptedAt && !(next.status === "UNVERIFIED" && previous.status === "LATEST"));
}
export const useProviders = create<ProvidersState>()((set, get) => ({
  providers: [],
  realProviders: [],
  debugCatalog: builtinDebugCatalog(),
  healthByProvider: {},
  scanning: false,
  pending: [],
  lastScan: null,
  usagePending: [], usageActive: [], usageResults: {},
  initialized: false, initializing: false,
  usageVersions: {}, usageRevisions: { all: 0, providers: {} }, providerErrors: {}, usageErrors: {},
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
    if (get().usagePending.includes(id) || useSettings.getState().providerEnabled[id] === false) return;
    const sourcePath = useSettings.getState().providerPaths[id];
    const version = get().usageVersions[id] ?? 0;
    set((s) => ({ usagePending: [...s.usagePending, id], usageErrors: { ...s.usageErrors, [id]: undefined } }));
    let release: (() => void) | undefined;
    let consentGeneration: number | undefined;
    try {
      await useOnlineUsage.getState().load();
      const consent = useOnlineUsage.getState();
      consentGeneration = consent.generation;
      if (!consent.ready || !consent.allowOnlineUsageQuery || consent.busy) return;
      release = await usageQueue.acquire();
      const current = useOnlineUsage.getState();
      if ((get().usageVersions[id] ?? 0) !== version || !current.allowOnlineUsageQuery || current.busy || current.generation !== consent.generation || useSettings.getState().providerEnabled[id] === false || useSettings.getState().providerPaths[id] !== sourcePath) return;
      set((s) => ({ usageActive: [...s.usageActive, id] }));
      const result = await queryProviderQuota(id, automatic);
      const latest = useOnlineUsage.getState();
      if ((get().usageVersions[id] ?? 0) !== version || !latest.allowOnlineUsageQuery || latest.busy || latest.generation !== consent.generation || useSettings.getState().providerEnabled[id] === false || useSettings.getState().providerPaths[id] !== sourcePath) return;
      set((s) => {
        if (!acceptUsage(s.usageResults[id], result)) return {};
        // Projection for existing dispatch consumers. Card presentation uses the typed result.
        const snapshot = result.latestSuccess;
        const identityChanged = ["IDENTITY_CHANGED", "IDENTITY_UNKNOWN", "UNVERIFIED"].includes(result.status);
        const realProviders = s.realProviders.map((p) => {
          if (p.id !== id || !p.account || p.usageQuery?.backend) return p;
          const quantities = snapshot?.quantities.filter((q) => q.unit === "currency" && q.remaining != null) ?? [];
          return { ...p, account: { ...p.account,
            usage: identityChanged ? null : snapshot?.display ?? p.account.usage,
            balance: identityChanged ? null : quantities.length ? quantities.map((q) => `${q.currency} ${q.remaining}`).join(" · ") : p.account.balance,
          } };
        });
        return { usageResults: { ...s.usageResults, [id]: result }, realProviders,
          providers: debugCatalogOverlay(realProviders, s.debugCatalog, useSettings.getState().debugProvider) };
      });
      await getProviderUtilizationNow(id, 0).catch(() => {});
    } catch (e) {
      const current = useOnlineUsage.getState();
      if ((get().usageVersions[id] ?? 0) === version && current.allowOnlineUsageQuery && !current.busy && current.generation === consentGeneration && useSettings.getState().providerPaths[id] === sourcePath) set((s) => ({ usageErrors: { ...s.usageErrors, [id]: String(e) } }));
    } finally {
      release?.();
      set((s) => ({ usagePending: s.usagePending.filter((p) => p !== id), usageActive: s.usageActive.filter((p) => p !== id) }));
    }
  },

  verifyAntigravityKey: async () => {
    const id = "ANTIGRAVITY_CLI" as const;
    if (get().scanning || get().pending.includes(id)) return;
    set((s) => ({ providerErrors: { ...s.providerErrors, [id]: undefined }, pending: [...s.pending, id] }));
    try {
      const info = await verifyAntigravityApiKey();
      set((s) => {
        const realProviders = s.realProviders.some((p) => p.id === id)
          ? s.realProviders.map((p) => p.id === id ? info : p) : [...s.realProviders, info];
        return { realProviders, providers: debugCatalogOverlay(realProviders, s.debugCatalog, useSettings.getState().debugProvider) };
      });
    } catch (e) {
      set((s) => ({ providerErrors: { ...s.providerErrors, [id]: String(e) } }));
    } finally {
      set((s) => ({ pending: s.pending.filter((p) => p !== id) }));
    }
  },

  verifyReasonixKey: async () => {
    const id = "REASONIX" as const;
    if (get().scanning || get().pending.includes(id)) return;
    set((s) => ({ providerErrors: { ...s.providerErrors, [id]: undefined }, pending: [...s.pending, id] }));
    try {
      const info = await verifyReasonixApiKey();
      set((s) => {
        const realProviders = s.realProviders.some((p) => p.id === id)
          ? s.realProviders.map((p) => p.id === id ? info : p) : [...s.realProviders, info];
        return { realProviders, providers: debugCatalogOverlay(realProviders, s.debugCatalog, useSettings.getState().debugProvider) };
      });
    } catch (e) {
      set((s) => ({ providerErrors: { ...s.providerErrors, [id]: String(e) } }));
    } finally {
      set((s) => ({ pending: s.pending.filter((p) => p !== id) }));
    }
  },

  syncUsage: async (ids) => {
    await Promise.all(ids.map(async (id) => {
      const version = get().usageVersions[id] ?? 0;
      const result = await getProviderUsageState(id).catch(() => null);
      if (result && (get().usageVersions[id] ?? 0) === version) set((s) => acceptUsage(s.usageResults[id], result) ? { usageResults: { ...s.usageResults, [id]: result } } : {});
    }));
  },

  load: () => {
    if (get().initialized) return Promise.resolve();
    if (loadFlight) return loadFlight;
    set({ initializing: true, error: null });
    loadFlight = (async () => {
    await useOnlineUsage.getState().load();
    usageUnlisten ??= onUsageBindingInvalidated((revisions) => {
      const affected: ProviderId[] = [];
      set((s) => {
      const usageVersions = { ...s.usageVersions }, usageResults = { ...s.usageResults };
      const ids = new Set([...s.realProviders.map((p) => p.id), ...Object.keys(usageResults) as ProviderId[]]);
      for (const id of ids) {
        if (revisionOf(revisions, id) <= revisionOf(s.usageRevisions, id)) continue;
        affected.push(id);
        usageVersions[id] = (usageVersions[id] ?? 0) + 1;
        const prior = usageResults[id];
        if (prior) usageResults[id] = { ...prior, status: "UNVERIFIED", identityKey: null, fromCache: !!prior.latestSuccess };
      }
      return { usageRevisions: { all: Math.max(s.usageRevisions.all, revisions.all), providers: Object.fromEntries([...new Set([...Object.keys(s.usageRevisions.providers), ...Object.keys(revisions.providers)])].map((id) => [id, Math.max(s.usageRevisions.providers[id as ProviderId] ?? 0, revisions.providers[id as ProviderId] ?? 0)])) }, usageVersions, usageResults };
      });
      void get().syncUsage(affected);
    }).catch((e) => { usageUnlisten = null; throw e; });
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
    await get().syncUsage(get().realProviders.map((p) => p.id));
    await get().refresh();
    })().then(() => { set({ initialized: true }); }).catch((e) => { set({ error: String(e) }); }).finally(() => {
      loadFlight = null;
      set({ initializing: false });
    });
    return loadFlight;
  },

  refresh: () => {
    if (!get().initialized && !get().initializing) return get().load();
    if (scanFlight) return scanFlight;
    scanFlight = (async () => {
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

    set((s) => ({ scanning: true, error: null, pending: s.providers.map((p) => p.id) }));
    try {
      // The final list is authoritative: registry order, and drops CLIs removed from the whitelist.
      const providers = await refreshProviders(scanId, useSettings.getState().providerPaths);
      if (currentScan === scanId) {
        set({ realProviders: providers, providers: debugCatalogOverlay(providers, get().debugCatalog, useSettings.getState().debugProvider), lastScan: Date.now() });
        await get().syncUsage(providers.map((p) => p.id));
      }
    } catch (e) {
      set({ error: String(e) });
    } finally {
      if (currentScan === scanId) set({ scanning: false, pending: [] });
    }
    })().catch((e) => { set({ error: String(e), scanning: false, pending: [] }); }).finally(() => { scanFlight = null; });
    return scanFlight;
  },

  refreshOne: async (id) => {
    let updated: ProviderInfo | undefined;
    if (get().scanning || get().pending.includes(id)) return;
    set((s) => ({ providerErrors: { ...s.providerErrors, [id]: undefined }, pending: [...s.pending, id] }));
    try {
      const info = await refreshProvider(id, useSettings.getState().providerPaths[id]);
      updated = info;
      set((s) => { const realProviders = s.realProviders.map((p) => p.id === id ? info : p); return { realProviders, providers: debugCatalogOverlay(realProviders, s.debugCatalog, useSettings.getState().debugProvider) }; });
    } catch (e) {
      set((s) => ({ providerErrors: { ...s.providerErrors, [id]: String(e) } }));
    } finally {
      set((s) => ({ pending: s.pending.filter((p) => p !== id) }));
    }
    if (updated) await get().syncUsage([id]);
    const consent = useOnlineUsage.getState();
    if (updated && canQueryUsage(updated) && consent.ready && consent.allowOnlineUsageQuery && !consent.busy) await get().refreshQuota(id);
  },
}));

// Debug overlays do not authorize online queries.
useSettings.subscribe((s, prev) => {
  if (s.debugProvider !== prev.debugProvider) {
    const store = useProviders.getState();
    useProviders.setState({ providers: debugCatalogOverlay(store.realProviders, store.debugCatalog, s.debugProvider) });
  }
});
