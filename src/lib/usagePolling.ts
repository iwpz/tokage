import { canQueryUsage, usageDue } from "./usagePolicy";
import { useOnlineUsage } from "../stores/onlineUsage";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";

/** App lifetime, not settings-page lifetime. Native permission/cache/backoff remain authoritative. */
export function startUsagePolling(): () => void {
  let stopped = false;
  const attempted = new Map<string, number>();
  let authChecked = Date.now();
  const tick = () => {
    if (stopped || document.visibilityState === "hidden") return;
    const store = useProviders.getState();
    if (store.scanning || store.pending.length) return;
    const now = Date.now();
    if (now - authChecked >= 300_000) {
      if (store.usagePending.length) return;
      authChecked = now;
      // Status detection is separate from quota consent; no model verification is triggered.
      void store.refresh();
      return;
    }
    const consent = useOnlineUsage.getState();
    if (!consent.ready || consent.busy || !consent.allowOnlineUsageQuery) return;
    for (const p of store.realProviders) {
      if (useSettings.getState().providerEnabled[p.id] === false || store.usagePending.includes(p.id) || !canQueryUsage(p)) continue;
      if (!usageDue(store.usageResults[p.id], attempted.get(p.id) ?? 0, now)) continue;
      attempted.set(p.id, now);
      void store.refreshQuota(p.id, true);
    }
  };
  const offProviders = useProviders.subscribe(tick);
  const offConsent = useOnlineUsage.subscribe(tick);
  const offSettings = useSettings.subscribe(tick);
  const timer = window.setInterval(tick, 15_000);
  window.addEventListener("focus", tick);
  document.addEventListener("visibilitychange", tick);
  tick();
  return () => { stopped = true; window.clearInterval(timer); offProviders(); offConsent(); offSettings(); window.removeEventListener("focus", tick); document.removeEventListener("visibilitychange", tick); };
}
