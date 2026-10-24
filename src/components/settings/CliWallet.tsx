import { useTranslation } from "react-i18next";
import type { AccountUsageResult, ProviderId } from "../../lib/ipc";

/** A named backend wallet never describes the currently selected CLI model. */
export function CliWallet({ result, provider = "OPENCODE" }: { result?: AccountUsageResult; provider?: ProviderId }) {
  const { t } = useTranslation();
  const snapshot = result?.latestSuccess;
  const wallet = snapshot?.scope.backend === "deepseek" && snapshot.scope.provider === provider ? snapshot : null;
  const labels: Record<string, string> = {
    total_balance: t("settings.cli.wallet.total"),
    granted_balance: t("settings.cli.wallet.granted"),
    topped_up_balance: t("settings.cli.wallet.toppedUp"),
  };
  return <div className="space-y-1 text-xs">
    <p className="text-subtle">{t("settings.cli.wallet.notice", { cli: provider === "REASONIX" ? "Reasonix" : "OpenCode" })}</p>
    {!wallet ? <p className="text-subtle">{t("settings.cli.wallet.empty")}</p> : <>
      <p className="text-subtle">{t(result?.status === "LATEST" ? (result.fromCache ? "settings.cli.wallet.cached" : "settings.cli.wallet.latest") : "settings.cli.wallet.history")} · {new Date(wallet.fetchedAt * 1000).toLocaleString()}</p>
      {wallet.available != null && <p>{t(wallet.available ? "settings.cli.wallet.available" : "settings.cli.wallet.unavailable")}</p>}
      <ul className="space-y-0.5">{wallet.quantities.filter((q) => q.unit === "currency" && q.remaining != null).map((q) => <li key={q.id} className="flex gap-2">
        <span>{labels[q.id.split("/").at(-1) ?? ""] ?? t("settings.cli.wallet.amount")}</span>
        <code>{q.currency} {q.remaining}</code>
      </li>)}</ul>
    </>}
  </div>;
}
