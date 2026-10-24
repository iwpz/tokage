import { create } from "zustand";
import { getOnlineUsagePermission, setOnlineUsagePermission, onOnlineUsagePermissionChanged, type OnlineUsagePermission } from "../lib/ipc";

interface State extends OnlineUsagePermission {
  ready: boolean;
  busy: boolean;
  error: string | null;
  load: () => Promise<void>;
  update: (allowed: boolean) => Promise<void>;
}
let listening: Promise<() => void> | null = null;
/** Native mirror only: intentionally absent from settings, DB capture and import. */
export const useOnlineUsage = create<State>()((set, get) => {
  const apply = (next: OnlineUsagePermission) => {
    if (next.generation >= get().generation) set({ ...next, ready: true });
  };
  return {
    allowOnlineUsageQuery: false, generation: 0, ready: false, busy: false, error: null,
    load: async () => {
      try {
        listening ??= onOnlineUsagePermissionChanged(apply).catch((e) => { listening = null; throw e; });
        await listening;
        apply(await getOnlineUsagePermission());
        set({ error: null });
      } catch (e) { set({ allowOnlineUsageQuery: false, ready: false, error: String(e) }); }
    },
    update: async (allowed) => {
      if (get().busy) return;
      // Stop frontend publication immediately while revocation is in flight.
      set({ busy: true, error: null, ...(!allowed ? { allowOnlineUsageQuery: false } : {}) });
      try { apply(await setOnlineUsagePermission(allowed)); }
      catch (e) {
        set({ allowOnlineUsageQuery: false, error: String(e) });
      } finally { set({ busy: false }); }
    },
  };
});
