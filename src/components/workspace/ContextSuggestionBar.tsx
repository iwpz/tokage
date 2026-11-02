import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation();

  if (plan.cooldownBlocked || ((plan.cooldownRemainingTurns ?? 0)>0 && plan.recommendedAction!=="CONTINUE")) {
    return <div role="status" className="px-3 py-2 text-xs text-warning">{t("workspace.context.cooldownBlocked")}</div>;
  }
  if (plan.recommendedAction === "CONTINUE") {
    return null;
  }

  const savingStr = `${Math.round(plan.estimatedSavingPercentage * 100)}%`;
  const diffCost = (
    plan.continueCostUsd -
    (plan.recommendedAction === "COMPACT" ? plan.compactCostUsd : plan.freshCostUsd)
  ).toFixed(2);
  const actionLabel =
    plan.recommendedAction === "COMPACT"
      ? t("workspace.context.suggestCompact")
      : t("workspace.context.suggestFresh");

  return (
    <div
      className={`flex items-center justify-between gap-3 px-3 py-2 bg-warning/10 border-y border-warning/20 text-warning dark:text-warning text-xs transition-all ${className}`}
      data-testid="context-suggestion-bar"
    >
      <div className="flex items-center gap-2 min-w-0">
        <Lightbulb className="w-4 h-4 text-warning dark:text-warning shrink-0 animate-bounce" />
        <div className="truncate">
          <span className="font-semibold mr-1.5">
            {t("workspace.context.suggestion")}
            {actionLabel}
          </span>
          <span className="text-warning dark:text-warning">
            {t("workspace.context.estimatedSaving", {
              saving: savingStr,
              amount: diffCost,
              explanation: plan.explanation,
            })}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-1.5 shrink-0">
        <Button
          size="sm"
          variant="primary"
          onClick={onApply}
          className="h-6 px-2 text-xs bg-warning hover:bg-warning text-white font-medium"
        >
          <Check className="w-3 h-3 mr-1" />
          {t("workspace.context.apply")}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={onDismiss}
          className="h-6 px-2 text-xs text-warning dark:text-warning hover:bg-warning/20"
        >
          <X className="w-3 h-3 mr-1" />
          {t("workspace.context.dismiss")}
        </Button>
      </div>
    </div>
  );
}
