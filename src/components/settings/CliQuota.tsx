import { useTranslation } from "react-i18next";
import type { AccountUsageResult } from "../../lib/ipc";
import { CliUsage } from "./CliUsage";
/** Scoped source data: never combine model quotas or invent a denominator/reset. */
export function CliQuota({ result }: { result: AccountUsageResult }) {
  const { t } = useTranslation();
  const s = result.latestSuccess;
  if (!s) return null;
  const names: Record<string, string> = {
    premium_interactions: t("settings.cli.quota.premium"), chat: t("settings.cli.quota.chat"), completions: t("settings.cli.quota.completions"),
    session: t("settings.cli.quota.session"), "week-all": t("settings.cli.quota.weekly"), included: t("settings.cli.quota.included"), auto: "Auto", api: "API",
  };

  return <div className="space-y-2">
    <p className="text-subtle">{t(result.status !== "LATEST" ? "settings.cli.wallet.history" : result.fromCache ? "settings.cli.wallet.cached" : "settings.cli.wallet.latest")} · {new Date(s.fetchedAt * 1000).toLocaleString()}</p>
    {s.display?.details ? <CliUsage usage={s.display} /> : s.windows.map((w) => <div key={w.id} className="flex flex-wrap items-center gap-x-3 gap-y-1">
      <span>{names[w.id] ?? (w.scope.startsWith("model:") || w.scope.startsWith("group:") ? w.scope.slice(w.scope.indexOf(":") + 1) : w.scope.startsWith("bucket:") ? `${w.scope.slice(7)} · ${w.period === "unlimited-credits" ? t("settings.cli.usage.unlimitedCredits") : w.period === "primary" ? t("settings.cli.usage.primaryWindow") : w.period === "secondary" ? t("settings.cli.usage.secondaryWindow") : w.period}` : w.period)}{w.windowMinutes ? ` · ${t(w.windowMinutes % 1440 === 0 ? "settings.cli.usage.days" : w.windowMinutes % 60 === 0 ? "settings.cli.usage.hours" : "settings.cli.usage.minutes", { n: w.windowMinutes % 1440 === 0 ? w.windowMinutes / 1440 : w.windowMinutes % 60 === 0 ? w.windowMinutes / 60 : w.windowMinutes })}` : ""}</span>
      {w.usedPercent != null && <span className="font-mono">{t("settings.cli.quota.remaining", { percent: Number((100 - w.usedPercent).toFixed(1)) })}</span>}
      {w.resetsAt != null && <span className="text-subtle">{t("settings.cli.quota.reset", { time: new Date(w.resetsAt * 1000).toLocaleString() })}</span>}
    </div>)}
    {s.quantities.filter((q) => q.unit !== "currency").map((q) => <p key={q.id} className="flex flex-wrap gap-2">
      <span>{q.used == null && q.remaining != null ? `${q.id.includes("/") ? q.id.slice(0, q.id.lastIndexOf("/")) + " · " : ""}${t("settings.cli.usage.remainingAmount")}` : names[q.id] ?? t("settings.cli.usage.plan")}</span>
      <code>{q.used ?? q.remaining ?? "—"}{q.limit != null ? ` / ${q.limit}` : ""}</code>
      <span>{t(q.unit === "copilot_billing_units" ? "settings.cli.quota.billingUnits" : q.unit === "premium_requests" ? "settings.cli.quota.premiumRequests" : q.unit === "credits" ? "settings.cli.quota.credits" : "settings.cli.quota.requests")}</span>
    </p>)}
  </div>;
}
