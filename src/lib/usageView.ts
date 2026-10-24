import type { AccountUsageResult, ProviderInfo } from "./ipc";
import { canQueryUsage } from "./usagePolicy";
/** Quota-source status is independent from CLI login and old local usage. */
export function usageView(p: ProviderInfo, result: AccountUsageResult | undefined, state: {
  detecting: boolean; pending: boolean; active: boolean; enabled: boolean; consentReady: boolean; allowed: boolean; transportError?: string;
}) {
  if (p.status === "NOT_INSTALLED") return null;
  if (state.detecting) return "detecting";
  if (!state.enabled) return "providerDisabled";
  if (!state.consentReady) return "initializing";
  if (!state.allowed) return "offline";
  if (state.pending) return state.active ? "querying" : "queued";
  if (state.transportError) return "transportFailed";
  if (!canQueryUsage(p)) return p.status === "NEEDS_LOGIN" && !p.usageQuery?.backend ? "needsLogin" : "unsupported";
  if (!result || result.status === "NOT_REQUESTED") return "notRequested";
  if (result.status === "UNVERIFIED") return "unverified";
  if (result.status === "IDENTITY_UNKNOWN") return "sourceUnknown";
  if (result.status === "IDENTITY_CHANGED") return "sourceChanged";
  if (result.status === "CANCELLED") return "cancelled";
  if (result.status === "DISABLED") return "notRequested";
  if (result.status === "UNSUPPORTED") return "unsupported";
  if (result.error === "AUTHENTICATION") return "accessRejected";
  if (result.status === "BACKOFF") return "backoff";
  if (result.status === "EXPIRED") return "expired";
  return null;
}
