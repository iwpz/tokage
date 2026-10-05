import { useEffect, useState } from "react";
import { Flame, Snowflake } from "lucide-react";
import { Badge } from "../ui";

interface CacheCountdownProps {
  /** Remaining warm cache TTL in seconds. */
  remainingSeconds: number;
  /** Whether cache is currently warm. */
  isWarm: boolean;
  className?: string;
}

export function CacheCountdown({
  remainingSeconds,
  isWarm,
  className = "",
}: CacheCountdownProps) {
  const [secondsLeft, setSecondsLeft] = useState(Math.max(0, Math.round(remainingSeconds)));

  useEffect(() => {
    setSecondsLeft(Math.max(0, Math.round(remainingSeconds)));
  }, [remainingSeconds]);

  useEffect(() => {
    if (!isWarm || secondsLeft <= 0) return;
    const timer = setInterval(() => {
      setSecondsLeft((prev) => Math.max(0, prev - 1));
    }, 1000);
    return () => clearInterval(timer);
  }, [isWarm, secondsLeft]);

  if (!isWarm || secondsLeft <= 0) {
    return (
      <span title="缓存已过期（下次请求为冷启动写入）">
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
    <span title="Prompt Cache 处于热状态，下次请求享受低价读取">
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
