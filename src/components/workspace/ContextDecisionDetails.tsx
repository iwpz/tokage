import { useTranslation } from "react-i18next";
import type { ResolvedContextInputs } from "../../lib/contextInputs";

/** Displays the same read-only preview used to offer the next-turn action. */
export function ContextDecisionDetails({ inputs }: { inputs: ResolvedContextInputs | null }) {
  const { t } = useTranslation();
  if (!inputs) return null;
  const { preview, params, cooldown } = inputs;
  const action = preview.cooldownBlocked || !cooldown.known
    ? t("workspace.context.decisionBlocked")
    : t(`workspace.context.decisionActions.${preview.recommendedAction}`);
  return <details className="border-b border-border px-3 py-2 text-xs text-muted">
    <summary className="cursor-pointer">{t("workspace.context.decisionDetails")} · {action}</summary>
    <div className="mt-2 space-y-1" data-testid="context-decision-details">
      <p>{preview.explanation}</p>
      <p>{t("workspace.context.decisionCosts", {
        continue: preview.continueCostUsd.toFixed(6), compact: preview.compactCostUsd.toFixed(6), fresh: preview.freshCostUsd.toFixed(6),
      })}</p>
      <p>{t("workspace.context.decisionForecast", { turns: params.remainingTurns, calls: params.apiCallsPerTurn, growth: params.growthPerTurn, threshold: (params.minSaving * 100).toFixed(1) })}</p>
      <p>{t("workspace.context.decisionRates", { read: params.cacheReadMultiplier, write: params.cacheWriteMultiplier, input: params.pIn, output: params.pOut })}</p>
      {!cooldown.known && <p>{t("workspace.context.cooldownUnknown")}</p>}
      {cooldown.known && (preview.cooldownRemainingTurns ?? 0) > 0 && <p>{t("workspace.context.cooldownRemaining", { count: preview.cooldownRemainingTurns })}</p>}
      {inputs.compactUnavailableReason && <p>{t("workspace.context.decisionUnavailable", { reason: inputs.compactUnavailableReason })}</p>}
      <p>{t("workspace.context.decisionEstimate")}</p>
    </div>
  </details>;
}
