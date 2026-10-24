import type { AccountUsageResult, ProviderInfo } from "./ipc";

export function canQueryUsage(p: ProviderInfo): boolean {
  const q = p.usageQuery;
  if (p.status === "NOT_INSTALLED" || q?.state !== "IDENTITY_REQUIRED" || q.method === "NONE") return false;
  if (q.backend) return true;
  return p.status !== "NEEDS_LOGIN" && p.account?.official !== false && !(q.authentication === "OFFICIAL_SUBSCRIPTION" && p.account?.billing === "PAY_AS_YOU_GO");
}
export function usageDue(result: AccountUsageResult | undefined, lastAttempt: number, now: number): boolean {
  if (now - lastAttempt < 60_000) return false;
  if (!result) return true;
  if (result.nextRetryAt != null && result.nextRetryAt * 1000 > now) return false;
  if (result.error === "AUTHENTICATION" && result.nextRetryAt == null) return false;
  if (result.status === "LATEST" && result.latestSuccess && result.latestSuccess.validUntil * 1000 > now) return false;
  if (result.nextRetryAt != null && ["FAILED", "BACKOFF", "CANCELLED"].includes(result.status)) return true;
  if (["IDENTITY_UNKNOWN", "IDENTITY_CHANGED", "FAILED", "CANCELLED"].includes(result.status) && result.attemptedAt * 1000 + 300_000 > now) return false;
  return result.status !== "UNSUPPORTED" && result.status !== "IDENTITY_CHANGED";
}
