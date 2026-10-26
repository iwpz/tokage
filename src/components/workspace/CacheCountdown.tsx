import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Flame, Snowflake, Database, Sparkles } from "lucide-react";
import { Badge } from "../ui";
import type { CacheType } from "../../lib/cachePolicy";

export interface CacheCountdownProps {
  /** Architecture cache type of the current active session/model. */
  cacheType?: CacheType;
  /** Remaining warm cache TTL in seconds (for ephemeral cache). */
  remainingSeconds?: number;
  /** Whether cache is currently warm. */
  isWarm?: boolean;
  /** Name of the active model for context in tooltips. */
  modelName?: string;
  className?: string;
}

export function CacheCountdown({
  cacheType = "ephemeral",
  remainingSeconds = 0,
  isWarm = false,
  modelName,
  className = "",
}: CacheCountdownProps) {
  const { t } = useTranslation();
  const [secondsLeft, setSecondsLeft] = useState(Math.max(0, Math.round(remainingSeconds)));
  const modelSuffix = modelName ? t("workspace.context.cache.modelSuffix", { model: modelName }) : "";

  useEffect(() => {
    setSecondsLeft(Math.max(0, Math.round(remainingSeconds)));
  }, [remainingSeconds]);

  useEffect(() => {
    if ((cacheType !== "ephemeral" && cacheType !== "disk_persistent") || !isWarm || secondsLeft <= 0) return;
    const timer = setInterval(() => {
      setSecondsLeft((prev) => Math.max(0, prev - 1));
    }, 1000);
    return () => clearInterval(timer);
  }, [cacheType, isWarm, secondsLeft]);

  // If the model does not support prompt caching, or no turn has run yet, hide the badge
  if (cacheType === "none") {
    return null;
  }

  // Persistent storage is not proof of an unexpired cache entry.
  if (cacheType === "disk_persistent" && isWarm && secondsLeft > 0) {
    return (
      <span title={t("workspace.context.cache.persistentTitle", { model: modelSuffix })}>
        <Badge
          variant="success"
          size="sm"
          className={`inline-flex items-center gap-1 font-mono text-xs ${className}`}
        >
          <Database className="w-3 h-3 text-emerald-500" />
          <span>{t("workspace.context.cache.persistent")}</span>
        </Badge>
      </span>
    );
  }

  // Implicit / opportunistic caching (e.g. Gemini, Qwen): backend managed, no fixed TTL countdown
  if (cacheType === "implicit") {
    return (
      <span title={t("workspace.context.cache.implicitTitle", { model: modelSuffix })}>
        <Badge
          variant="muted"
          size="sm"
          className={`inline-flex items-center gap-1 font-mono text-xs ${className}`}
        >
          <Sparkles className="w-3 h-3 text-purple-400" />
          <span>{t("workspace.context.cache.implicit")}</span>
        </Badge>
      </span>
    );
  }

  // Ephemeral TTL cache (e.g. Claude 5min, Codex 30min)
  if (!isWarm || secondsLeft <= 0) {
    return (
      <span title={t("workspace.context.cache.coldTitle", { model: modelSuffix })}>
        <Badge
          variant="muted"
          size="sm"
          className={`inline-flex items-center gap-1 font-mono text-xs ${className}`}
        >
          <Snowflake className="w-3 h-3 text-sky-400" />
          <span>{t("workspace.context.cache.cold")}</span>
        </Badge>
      </span>
    );
  }

  const mins = Math.floor(secondsLeft / 60);
  const secs = secondsLeft % 60;
  const timeStr = `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;

  return (
    <span title={t("workspace.context.cache.hotTitle", { model: modelSuffix, time: timeStr })}>
      <Badge
        variant="warning"
        size="sm"
        className={`inline-flex items-center gap-1 font-mono text-xs ${className}`}
      >
        <Flame className="w-3 h-3 text-amber-500 animate-pulse" />
        <span>{t("workspace.context.cache.hot", { time: timeStr })}</span>
      </Badge>
    </span>
  );
}
