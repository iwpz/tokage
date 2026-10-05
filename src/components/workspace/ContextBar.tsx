import { useMemo } from "react";
import { Layers, Minimize2 } from "lucide-react";
import { CacheCountdown } from "./CacheCountdown";
import { Button } from "../ui";

interface ContextBarProps {
  /** Current context token count (history + baseline). */
  tokens: number;
  /** Max effective context tokens (e.g. 128_000). */
  maxTokens?: number;
  /** Whether the cache is warm right now. */
  isWarm?: boolean;
  /** Remaining cache TTL seconds. */
  ttlRemainingSeconds?: number;
  /** Callback to trigger compaction manually. */
  onTriggerCompact?: () => void;
  className?: string;
}

export function ContextBar({
  tokens,
  maxTokens = 128_000,
  isWarm = true,
  ttlRemainingSeconds = 240,
  onTriggerCompact,
  className = "",
}: ContextBarProps) {
  const percentage = useMemo(() => {
    if (maxTokens <= 0) return 0;
    return Math.min(100, Math.round((tokens / maxTokens) * 100));
  }, [tokens, maxTokens]);

  const colorClass = useMemo(() => {
    if (percentage >= 85) return "bg-red-500 text-red-500";
    if (percentage >= 65) return "bg-amber-500 text-amber-500";
    return "bg-emerald-500 text-emerald-500";
  }, [percentage]);

  const formattedTokens = useMemo(() => {
    if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
    if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}K`;
    return String(tokens);
  }, [tokens]);

  const formattedMax = useMemo(() => {
    if (maxTokens >= 1_000_000) return `${(maxTokens / 1_000_000).toFixed(0)}M`;
    if (maxTokens >= 1_000) return `${(maxTokens / 1_000).toFixed(0)}K`;
    return String(maxTokens);
  }, [maxTokens]);

  return (
    <div
      className={`flex items-center justify-between gap-3 px-3 py-1.5 text-xs bg-neutral-50 dark:bg-neutral-900 border-b border-neutral-200 dark:border-neutral-800 transition-colors ${className}`}
      data-testid="context-bar"
    >
      <div className="flex items-center gap-2 min-w-0">
        <Layers className="w-3.5 h-3.5 text-neutral-400 shrink-0" />
        <span className="font-medium text-neutral-600 dark:text-neutral-300 whitespace-nowrap">
          上下文用量
        </span>
        <div
          role="progressbar"
          aria-valuenow={percentage}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuetext={`上下文用量 ${formattedTokens} / ${formattedMax} (${percentage}%)`}
          className="w-24 sm:w-36 h-1.5 bg-neutral-200 dark:bg-neutral-800 rounded-full overflow-hidden shrink-0"
        >
          <div
            className={`h-full rounded-full transition-all duration-300 ${colorClass.split(" ")[0]}`}
            style={{ width: `${percentage}%` }}
          />
        </div>
        <span className="font-mono text-neutral-500 dark:text-neutral-400 whitespace-nowrap">
          {formattedTokens} / {formattedMax} ({percentage}%)
        </span>
      </div>

      <div className="flex items-center gap-2 shrink-0">
        <CacheCountdown
          remainingSeconds={ttlRemainingSeconds}
          isWarm={isWarm}
        />
        {onTriggerCompact && tokens > 30_000 && (
          <Button
            size="sm"
            variant="ghost"
            onClick={onTriggerCompact}
            className="h-6 px-2 text-xs text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-100"
            title="执行上下文压缩"
          >
            <Minimize2 className="w-3 h-3 mr-1" />
            压缩
          </Button>
        )}
      </div>
    </div>
  );
}
