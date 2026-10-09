import { describe, it, expect } from "vitest";
import {
  IN_WINDOW_CONTEXT_DEFAULTS,
  mockPlanInWindowContext,
  resolveInWindowContextParams,
} from "./contextPlanDefaults";

describe("contextPlanDefaults (M1.a3)", () => {
  it("fills omitted optional fields from Rust-aligned defaults", () => {
    const p = resolveInWindowContextParams({
      historyTokens: 90_000,
      elapsedSecondsSinceLastTurn: 720,
    });
    expect(p.minSaving).toBe(IN_WINDOW_CONTEXT_DEFAULTS.minSaving);
    expect(p.pIn).toBe(IN_WINDOW_CONTEXT_DEFAULTS.pIn);
    expect(p.compactOutputRatio).toBe(IN_WINDOW_CONTEXT_DEFAULTS.compactOutputRatio);
    expect(p.cacheTtlSeconds).toBe(300);
    expect(p.cacheType).toBe("standard");
  });

  it("preserves explicit overrides including cacheAware-off", () => {
    const p = resolveInWindowContextParams({
      historyTokens: 10_000,
      elapsedSecondsSinceLastTurn: 10,
      cacheTtlSeconds: 0,
      cacheType: "none",
      minSaving: 0.2,
    });
    expect(p.cacheTtlSeconds).toBe(0);
    expect(p.cacheType).toBe("none");
    expect(p.minSaving).toBe(0.2);
  });

  it("mock plans COMPACT for cold long history (docs/05 §5.1)", () => {
    const plan = mockPlanInWindowContext({
      baselineTokens: 25_000,
      historyTokens: 90_000,
      remainingTurns: 4,
      apiCallsPerTurn: 6,
      growthPerTurn: 8_000,
      outputPerTurn: 1_000,
      elapsedSecondsSinceLastTurn: 720,
    });
    expect(plan.recommendedAction).toBe("COMPACT");
    expect(plan.warmNow).toBe(false);
    expect(plan.estimatedSavingPercentage).toBeGreaterThanOrEqual(0.15);
  });

  it("mock plans CONTINUE for hot short history", () => {
    const plan = mockPlanInWindowContext({
      baselineTokens: 25_000,
      historyTokens: 15_000,
      remainingTurns: 2,
      apiCallsPerTurn: 6,
      growthPerTurn: 8_000,
      outputPerTurn: 1_000,
      elapsedSecondsSinceLastTurn: 60,
    });
    expect(plan.recommendedAction).toBe("CONTINUE");
    expect(plan.warmNow).toBe(true);
  });
});
