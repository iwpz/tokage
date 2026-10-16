import { useTranslation } from "react-i18next";
import support from "../lib/reliabilitySupport.json";

/** Probe readiness and adapter compatibility never certify a reliability milestone. */
export function ProviderReliability({ providerId, version }: { providerId: string; version: string | null }) {
  const { t } = useTranslation();
  const target = support.targets.find((entry) => entry.provider === providerId);
  return (
    <div className="mt-1 text-[11px] text-subtle">
      <p>{t("reliability.probeOnly")}</p>
      {target ? <>
        <p>{t("reliability.target", { platform: target.platform, version: target.version, model: target.model })}</p>
        <p>{t(target.authentication === "officialSubscription" ? "reliability.officialSubscription" : "reliability.deepseekApiKey")} · {t("reliability.sessionFullAuto")} · {t("reliability.worktree")}</p>
        <p>{t("reliability.evidence", { date: target.historicalDate })}</p>
        {version !== target.version && <p className="text-warning">{t("reliability.versionUnverified", { version: version ?? "—" })}</p>}
        <p>{target.naturalDesktop === "passed" && version === target.version
          ? t("reliability.naturalPassed", { date: target.naturalDate, build: target.osBuild })
          : t("reliability.pending")}</p>
      </> : <p>{t("reliability.outside")}</p>}
    </div>
  );
}
