import { useEffect, useState } from "react";
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
  const [secondsLeft, setSecondsLeft] = useState(Math.max(0, Math.round(remainingSeconds)));

  useEffect(() => {
    setSecondsLeft(Math.max(0, Math.round(remainingSeconds)));
  }, [remainingSeconds]);

  useEffect(() => {
    if (cacheType !== "ephemeral" || !isWarm || secondsLeft <= 0) return;
    const timer = setInterval(() => {
      setSecondsLeft((prev) => Math.max(0, prev - 1));
    }, 1000);
    return () => clearInterval(timer);
  }, [cacheType, isWarm, secondsLeft]);

  // If the model does not support prompt caching, or no turn has run yet, hide the badge
  if (cacheType === "none") {
    return null;
  }

  // Disk-persistent caching (e.g. DeepSeek): always warm across long intervals
  if (cacheType === "disk_persistent") {
    return (
      <span title={`持久化缓存${modelName ? ` (${modelName})` : ""}：服务端长期保留前缀缓存，跨轮次保持热状态`}>
        <Badge
          variant="success"
          size="sm"
          className={`inline-flex items-center gap-1 font-mono text-xs ${className}`}
        >
          <Database className="w-3 h-3 text-emerald-500" />
          <span>持久缓存</span>
        </Badge>
      </span>
    );
  }

  // Implicit / opportunistic caching (e.g. Gemini, Qwen): backend managed, no fixed TTL countdown
  if (cacheType === "implicit") {
    return (
      <span title={`自动上下文缓存${modelName ? ` (${modelName})` : ""}：服务端根据前缀一致性自动匹配`}>
        <Badge
          variant="muted"
          size="sm"
          className={`inline-flex items-center gap-1 font-mono text-xs ${className}`}
        >
          <Sparkles className="w-3 h-3 text-purple-400" />
          <span>自动缓存</span>
        </Badge>
      </span>
    );
  }

  // Ephemeral TTL cache (e.g. Claude 5min, Codex 30min)
  if (!isWarm || secondsLeft <= 0) {
    return (
      <span title={`缓存已过期${modelName ? ` (${modelName})` : ""}：下次请求为冷启动写入`}>
        <Badge
          variant="muted"
          size="sm"
          className={`inline-flex items-center gap-1 font-mono text-xs ${className}`}
        >
          <Snowflake className="w-3 h-3 text-sky-400" />
          <span>冷缓存</span>
        </Badge>
      </span>
    );
  }

  const mins = Math.floor(secondsLeft / 60);
  const secs = secondsLeft % 60;
  const timeStr = `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;

  return (
    <span title={`Prompt Cache 处于热状态${modelName ? ` (${modelName})` : ""}，剩余 ${timeStr}：下次请求享受低价读取`}>
      <Badge
        variant="warning"
        size="sm"
        className={`inline-flex items-center gap-1 font-mono text-xs ${className}`}
      >
        <Flame className="w-3 h-3 text-amber-500 animate-pulse" />
        <span>热缓存 {timeStr}</span>
      </Badge>
    </span>
  );
}
