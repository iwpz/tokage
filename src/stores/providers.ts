import { create } from "zustand";
import { persist } from "zustand/middleware";
import { onProviderDetected, refreshProvider, refreshProviders, type ProviderId, type ProviderInfo } from "../lib/ipc";
import { useSettings } from "./settings";

interface ProvidersState {
  /** Last known results; restored from cache on startup, then replaced row by row. */
  providers: ProviderInfo[];
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
}

let unlisten: Promise<() => void> | null = null;
let currentScan: string | null = null;

export const useProviders = create<ProvidersState>()(
  persist(
    (set, get) => ({
      providers: [],
      scanning: false,
      pending: [],
      lastScan: null,
      error: null,

      load: async () => {
        await get().refresh();
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
    }),
    {
      name: "tokage.providers",
      // Bump when ProviderInfo changes shape; older caches are dropped and rebuilt by the scan.
      version: 2,
      migrate: () => ({ providers: [], lastScan: null }),
      partialize: (s) => ({ providers: s.providers, lastScan: s.lastScan }),
    },
  ),
);
