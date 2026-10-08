import { create } from "zustand";
import {
  getProviderUtilizationNow,
  listProviderHealth,
  onProviderDetected,
  onProviderMelted,
  onProviderRecovered,
  queryProviderQuota,
  recoverProviderHealth,
  refreshProvider,
  refreshProviders,
  type ProviderHealthRow,
  type ProviderId,
  type ProviderInfo,
  type ProviderMeltedEvent,
  type ProviderRecoveredEvent,
} from "../lib/ipc";
import { useSettings } from "./settings";

interface ProvidersState {
  /** Last known results; restored from cache on startup, then replaced row by row. */
  providers: ProviderInfo[];
  /** provider_health rows keyed by provider name (F15.e). */
  healthByProvider: Record<string, ProviderHealthRow>;
  scanning: boolean;
  /** CLIs whose fresh result hasn't arrived yet in the current scan. */
  pending: ProviderId[];
  lastScan: number | null;
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
  refreshQuota: (id: ProviderId) => Promise<void>;
  /** True when provider is MELTED (not selectable). HALF_OPEN remains runnable for probe. */
  isMelted: (providerName: string) => boolean;
  healthOf: (providerName: string) => ProviderHealthRow | undefined;
}

let unlisten: Promise<() => void> | null = null;
let healthUnlisten: Promise<() => void> | null = null;
let currentScan: string | null = null;
let quotaTimer: ReturnType<typeof setTimeout> | null = null;
let lastActivityAt = Date.now();

function clearQuotaTimer() {
  if (quotaTimer) {
    clearTimeout(quotaTimer);
    quotaTimer = null;
  }
}

/** F15.e: adaptive quota polling when `activeQuotaQuery` is on. */
function scheduleQuotaPolling() {
  clearQuotaTimer();
  const settings = useSettings.getState();
  if (!settings.activeQuotaQuery) return;

  const tick = async () => {
    const s = useSettings.getState();
    if (!s.activeQuotaQuery) return;
    // Pause when the app has been idle > 10 minutes.
    if (Date.now() - lastActivityAt > 10 * 60 * 1000) {
      quotaTimer = setTimeout(() => void tick(), 60_000);
      return;
    }
    const providers = useProviders.getState().providers;
    let maxUtil = 0;
    for (const p of providers) {
      if (p.status === "NOT_INSTALLED") continue;
      try {
        const snap = await queryProviderQuota(p.id);
        const pct = snap?.windows?.reduce((m, w) => Math.max(m, w.usedPercent ?? 0), 0) ?? 0;
        const util = pct > 1 ? pct / 100 : pct;
        maxUtil = Math.max(maxUtil, util);
        // Soft melt when utilization ≥ meltThreshold (docs/06 §6.1).
        if (util >= s.meltThreshold && !useProviders.getState().isMelted(p.id)) {
          const { tripProviderMelt } = await import("../lib/ipc");
          await tripProviderMelt(p.id, "QUOTA_SOFT");
        }
      } catch {
        /* best-effort */
      }
    }
    const delayMs = maxUtil >= 0.8 ? 2 * 60_000 : 5 * 60_000;
    quotaTimer = setTimeout(() => void tick(), delayMs);
  };
  void tick();
}

if (typeof window !== "undefined") {
  const bump = () => {
    lastActivityAt = Date.now();
  };
  window.addEventListener("pointerdown", bump);
  window.addEventListener("keydown", bump);
}

export const useProviders = create<ProvidersState>()((set, get) => ({
  providers: [],
  healthByProvider: {},
  scanning: false,
  pending: [],
  lastScan: null,
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

  refreshQuota: async (id) => {
    await queryProviderQuota(id);
    try {
      await getProviderUtilizationNow(id, 0);
    } catch {
      /* optional */
    }
  },

  load: async () => {
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

    await get().refresh();
    scheduleQuotaPolling();
  },

  refresh: async () => {
    if (get().scanning) return;
    const scanId = crypto.randomUUID();
    currentScan = scanId;
    unlisten ??= onProviderDetected(({ scanId: id, provider }) => {
      if (id !== currentScan) return;
      set((s) => {
        const i = s.providers.findIndex((p) => p.id === provider.id);
        const providers =
          i === -1 ? [...s.providers, provider] : s.providers.map((p, j) => (j === i ? provider : p));
        return { providers, pending: s.pending.filter((p) => p !== provider.id) };
      });
    });
    await unlisten;

    set((s) => ({ scanning: true, error: null, pending: s.providers.map((p) => p.id) }));
    try {
      // The final list is authoritative: registry order, and drops CLIs removed from the whitelist.
      const providers = await refreshProviders(scanId, useSettings.getState().providerPaths);
      if (currentScan === scanId) set({ providers, lastScan: Date.now() });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      if (currentScan === scanId) set({ scanning: false, pending: [] });
    }
  },

  refreshOne: async (id) => {
    if (get().scanning || get().pending.includes(id)) return;
    set((s) => ({ error: null, pending: [...s.pending, id] }));
    try {
      const info = await refreshProvider(id, useSettings.getState().providerPaths[id]);
      set((s) => ({ providers: s.providers.map((p) => (p.id === id ? info : p)) }));
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set((s) => ({ pending: s.pending.filter((p) => p !== id) }));
    }
  },
}));

// Reschedule quota polling when the setting flips.
useSettings.subscribe((s, prev) => {
  if (s.activeQuotaQuery !== prev.activeQuotaQuery || s.meltThreshold !== prev.meltThreshold) {
    scheduleQuotaPolling();
  }
});
