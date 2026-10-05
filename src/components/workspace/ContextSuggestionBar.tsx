import { Lightbulb, Check, X } from "lucide-react";
import type { ContextActionPlan } from "../../lib/ipc";
import { Button } from "../ui";

interface ContextSuggestionBarProps {
  plan: ContextActionPlan;
  onApply: () => void;
  onDismiss: () => void;
  className?: string;
}

export function ContextSuggestionBar({
  plan,
  onApply,
  onDismiss,
  className = "",
}: ContextSuggestionBarProps) {
  if (plan.recommendedAction === "CONTINUE") {
    return null;
  }

  const savingStr = `${Math.round(plan.estimatedSavingPercentage * 100)}%`;
  const diffCost = (plan.continueCostUsd - (plan.recommendedAction === "COMPACT" ? plan.compactCostUsd : plan.freshCostUsd)).toFixed(2);

  return (
    <div
      className={`flex items-center justify-between gap-3 px-3 py-2 bg-amber-500/10 border-y border-amber-500/20 text-amber-900 dark:text-amber-200 text-xs transition-all ${className}`}
      data-testid="context-suggestion-bar"
    >
      <div className="flex items-center gap-2 min-w-0">
        <Lightbulb className="w-4 h-4 text-amber-600 dark:text-amber-400 shrink-0 animate-bounce" />
        <div className="truncate">
          <span className="font-semibold mr-1.5">
            上下文建议：{plan.recommendedAction === "COMPACT" ? "压缩上下文" : "开启新窗口"}
          </span>
          <span className="text-amber-700 dark:text-amber-300">
            预计节省 <span className="font-bold text-amber-800 dark:text-amber-100">{savingStr}</span> (${diffCost})。{plan.explanation}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-1.5 shrink-0">
        <Button
          size="sm"
          variant="primary"
          onClick={onApply}
          className="h-6 px-2 text-xs bg-amber-600 hover:bg-amber-700 text-white font-medium"
        >
          <Check className="w-3 h-3 mr-1" />
          应用
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={onDismiss}
          className="h-6 px-2 text-xs text-amber-800 dark:text-amber-300 hover:bg-amber-500/20"
        >
          <X className="w-3 h-3 mr-1" />
          忽略
        </Button>
      </div>
    </div>
  );
}
