import i18n from "../i18n";
/**
 * Defaults aligned with `InWindowContextParams::default` /
 * `WindowReuseParams::default` in tokage-context (M1.a3).
 * Non-Tauri mocks merge these so omitted optional fields match Rust serde(default).
 */

export const IN_WINDOW_CONTEXT_DEFAULTS = {
  baselineTokens: 25_000,
  historyTokens: 90_000,
  remainingTurns: 4,
  apiCallsPerTurn: 6,
  growthPerTurn: 8_000,
  outputPerTurn: 1_000,
  compactOutputRatio: 0.06,
  reworkPhi: 0.3,
  minSaving: 0.15,
  cacheReadMultiplier: 0.1,
  cacheWriteMultiplier: 1.25,
  cacheTtlSeconds: 300,
  elapsedSecondsSinceLastTurn: 720,
  cacheType: "standard",
  implicitHitReliability: 1,
  effectiveContext: 128_000,
  contextWindow: 200_000,
  cliAutoCompactRatio: 0.92,
  turnsSinceLastCompact: 5,
  pIn: 3,
  pOut: 15,
} as const;

export type ResolvedInWindowContextParams = {
  [K in keyof typeof IN_WINDOW_CONTEXT_DEFAULTS]: number | string;
} & {
  baselineTokens: number;
  historyTokens: number;
  remainingTurns: number;
  apiCallsPerTurn: number;
  growthPerTurn: number;
  outputPerTurn: number;
  compactOutputRatio: number;
  reworkPhi: number;
  minSaving: number;
  cacheReadMultiplier: number;
  cacheWriteMultiplier: number;
  cacheTtlSeconds: number;
  elapsedSecondsSinceLastTurn: number;
  cacheType: string;
  implicitHitReliability: number;
  effectiveContext: number;
  contextWindow: number;
  cliAutoCompactRatio: number;
  turnsSinceLastCompact: number;
  pIn: number;
  pOut: number;
};

export type InWindowContextParamInput = Partial<ResolvedInWindowContextParams> & {
  historyTokens: number;
  elapsedSecondsSinceLastTurn: number;
};

/** Merge caller fields over Rust-aligned defaults (undefined keys keep default). */
export function resolveInWindowContextParams(
  params: InWindowContextParamInput,
): ResolvedInWindowContextParams {
  const out: ResolvedInWindowContextParams = { ...IN_WINDOW_CONTEXT_DEFAULTS };
  for (const key of Object.keys(IN_WINDOW_CONTEXT_DEFAULTS) as (keyof ResolvedInWindowContextParams)[]) {
    const v = params[key];
    if (v !== undefined) {
      (out as Record<string, number | string>)[key] = v;
    }
  }
  return out;
}

export type MockContextActionPlan = {
  recommendedAction: "CONTINUE" | "COMPACT" | "FRESH";
  trigger: "PLANNER" | "HARD_LIMIT" | "USER";
  continueCostUsd: number;
  compactCostUsd: number;
  freshCostUsd: number;
  estimatedSavingPercentage: number;
  warmNow: boolean;
  cacheTtlRemainingSeconds: number;
  continueDisqualified: boolean;
  explanation: string;
};

/**
 * Non-Tauri / vitest stand-in for `plan_context_action`.
 * Qualitatively matches docs/05 §5.1 fixtures: cold+long → COMPACT, hot+short → CONTINUE.
 */
export function mockPlanInWindowContext(
  params: InWindowContextParamInput,
): MockContextActionPlan {
  const p = resolveInWindowContextParams(params);

  const effectiveTtl = p.cacheTtlSeconds * 0.9;
  const warmNow =
    p.cacheType === "disk_persistent"
      ? true
      : p.cacheType === "none" || p.cacheTtlSeconds <= 0
        ? false
        : p.elapsedSecondsSinceLastTurn < effectiveTtl;
  const cacheTtlRemainingSeconds = warmNow
    ? Math.max(0, effectiveTtl - p.elapsedSecondsSinceLastTurn)
    : 0;

  const projected =
    p.baselineTokens + p.historyTokens + p.remainingTurns * p.growthPerTurn;
  const hardLimit =
    Math.min(p.effectiveContext, p.contextWindow * p.cliAutoCompactRatio) * 0.9;
  const continueDisqualified = projected > hardLimit;

  // Same qualitative threshold as prior mock / §5.1: long cold history prefers COMPACT.
  const longHistory = p.historyTokens > 60_000;
  const shouldCompact = continueDisqualified || (longHistory && !warmNow);

  if (shouldCompact && 0.38 >= p.minSaving) {
    return {
      recommendedAction: "COMPACT",
      trigger: continueDisqualified ? "HARD_LIMIT" : "PLANNER",
      continueCostUsd: 0.61,
      compactCostUsd: 0.38,
      freshCostUsd: 0.45,
      estimatedSavingPercentage: 0.38,
      warmNow,
      cacheTtlRemainingSeconds,
      continueDisqualified,
      explanation: continueDisqualified
        ? i18n.t("contextDefaults.nearHardLimit")
        : i18n.t("contextDefaults.compactSavings"),
    };
  }

  return {
    recommendedAction: "CONTINUE",
    trigger: "PLANNER",
    continueCostUsd: 0.08,
    compactCostUsd: 0.07,
    freshCostUsd: 0.09,
    estimatedSavingPercentage: 0.11,
    warmNow,
    cacheTtlRemainingSeconds,
    continueDisqualified: false,
    explanation: i18n.t("contextDefaults.continueHere"),
  };
}
