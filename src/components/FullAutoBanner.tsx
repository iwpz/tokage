import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Flame, X } from "lucide-react";
import { useAuth } from "../stores/auth";
import { Button } from "./ui";

export function FullAutoBanner({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { fullAutoGrants, isFullAutoActive, revokeFullAuto } = useAuth();
  const [, setTick] = useState(0);

  const active = isFullAutoActive(sessionId);
  const grant = fullAutoGrants[sessionId];

  // Refresh countdown every 5 seconds
  useEffect(() => {
    if (!active || !grant?.expiresAt) return;
    const timer = setInterval(() => {
      if (!useAuth.getState().isFullAutoActive(sessionId)) {
        clearInterval(timer);
      }
      setTick((v) => v + 1);
    }, 5000);
    return () => clearInterval(timer);
  }, [active, grant?.expiresAt, sessionId]);

  if (!active) return null;

  const formatRemaining = () => {
    if (!grant || grant.expiresAt == null) {
      return t("settings.security.fullAutoBanner.validSession");
    }
    const diff = Math.max(0, grant.expiresAt - Date.now());
    const totalMinutes = Math.floor(diff / (60 * 1000));
    if (totalMinutes < 60) {
      return t("settings.security.fullAutoBanner.remainingMinutes", { minutes: Math.max(1, totalMinutes) });
    }
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    return t("settings.security.fullAutoBanner.remainingHoursMinutes", { hours, minutes });
  };

  return (
    <div
      role="alert"
      className="sticky top-0 z-20 flex items-center justify-between gap-3 border-b border-warning/40 bg-warning-soft/60 px-4 py-2 text-xs backdrop-blur-sm"
    >
      <div className="flex min-w-0 items-center gap-2">
        <Flame className="size-4 shrink-0 text-warning animate-pulse" />
        <span className="font-semibold text-warning shrink-0">
          {t("settings.security.fullAutoBanner.title")}
        </span>
        <span className="text-subtle hidden md:inline truncate">
          {t("settings.security.fullAutoBanner.desc")}
        </span>
      </div>

      <div className="flex shrink-0 items-center gap-2.5">
        <span className="font-mono text-[11px] font-medium text-warning bg-warning/15 px-2 py-0.5 rounded">
          {formatRemaining()}
        </span>
        <Button
          size="sm"
          variant="ghost"
          className="h-6 px-2 text-xs text-muted hover:text-danger hover:bg-danger-soft"
          onClick={() => revokeFullAuto(sessionId)}
        >
          <X className="size-3.5 mr-1" />
          {t("settings.security.fullAutoBanner.exitBtn")}
        </Button>
      </div>
    </div>
  );
}
