import { useTranslation } from "react-i18next";
import type { UsageSnapshot } from "../../lib/ipc";
import { cn } from "../../lib/cn";

function windowLabel(minutes: number, t: (key: any, opts?: any) => string) {
  if (minutes === 0) return t("settings.cli.usage.plan");
  if (minutes === 10080) return t("settings.cli.usage.weekly");
  if (minutes % 1440 === 0) return t("settings.cli.usage.days", { n: minutes / 1440 });
  if (minutes % 60 === 0) return t("settings.cli.usage.hours", { n: minutes / 60 });
  return t("settings.cli.usage.minutes", { n: minutes });
}

function formatIn(seconds: number, t: (key: any, opts?: any) => string) {
  const m = Math.max(1, Math.round(seconds / 60));
  if (m < 60) return t("settings.cli.usage.inMinutes", { n: m });
  const h = Math.floor(m / 60);
  if (h < 48) return t("settings.cli.usage.inHours", { h, m: m % 60 });
  return t("settings.cli.usage.inDays", { d: Math.floor(h / 24), h: h % 24 });
}

/** Rolling quota windows as meters. A window whose reset time has passed shows as reset. */
export function CliUsage({ usage }: { usage: UsageSnapshot }) {
  const { windows, observedAt, details } = usage;
  const { t, i18n } = useTranslation();
  const now = Date.now() / 1000;
  return (
    <div className="min-w-0 space-y-1.5">
      {details && (
        <>
          <p className="text-muted">{details.plan}{details.resetLabel && ` · ${details.resetLabel}`}</p>
          {details.rows.map((row) => (
            <div key={row.kind} className="flex flex-wrap items-center gap-2">
              <span className="w-20 shrink-0 text-muted">{t(`settings.cli.usage.rows.${row.kind}`)}</span>
              {row.usedPercent != null ? (
                <>
                  <div role="meter" aria-label={t(`settings.cli.usage.rows.${row.kind}`)} aria-valuemin={0} aria-valuemax={100}
                    aria-valuenow={row.usedPercent} className="h-1.5 w-24 overflow-hidden rounded-full bg-hover">
                    <div className="h-full bg-accent" style={{ width: `${Math.min(100, Math.max(0, row.usedPercent))}%` }} />
                  </div>
                  <span className="font-mono tabular-nums">{row.usedPercent}%</span>
                </>
              ) : <span>{row.value === "Disabled" ? t("settings.cli.usage.disabled") : row.value}</span>}
            </div>
          ))}
        </>
      )}
      {!details && windows.map((w) => {
        const reset = w.resetsAt != null && w.resetsAt <= now;
        const used = reset ? 0 : Math.min(100, Math.max(0, w.usedPercent));
        const label = windowLabel(w.windowMinutes, t);
        const tone = used >= 90 ? "bg-danger" : used >= 70 ? "bg-warning" : "bg-accent";
        return (
          <div key={w.windowMinutes} className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
            <span className="w-12 shrink-0 text-muted">{label}</span>
            <div
              role="meter"
              aria-label={label}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={used}
              className="h-1.5 w-24 shrink-0 overflow-hidden rounded-full bg-hover"
            >
              <div className={cn("h-full rounded-full", tone)} style={{ width: `${used}%` }} />
            </div>
            <span className="w-9 shrink-0 text-right font-mono tabular-nums">{Math.round(used)}%</span>
            <span className="whitespace-nowrap text-subtle">
              {reset
                ? t("settings.cli.usage.resetSince")
                : w.resetsAt != null && t("settings.cli.usage.resets", { when: formatIn(w.resetsAt - now, t) })}
            </span>
          </div>
        );
      })}
      {observedAt && (
        <p className="text-subtle">
          {t(usage.source === "CLI_OUTPUT" ? "settings.cli.usage.queried" : "settings.cli.usage.observed", {
            time: new Date(observedAt).toLocaleString(i18n.language, {
              month: "short",
              day: "numeric",
              hour: "2-digit",
              minute: "2-digit",
            }),
          })}
        </p>
      )}
    </div>
  );
}
