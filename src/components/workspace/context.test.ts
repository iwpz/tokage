import { describe, it, expect } from "vitest";
import {
  planInWindowContext,
  evaluateWindowReuse,
  triageMainSessionMessage,
  buildCompact,
} from "../../lib/ipc";

describe("Frontend Context Lifecycle Engine (P8.m1)", () => {
  it("plans in-window context action with cold cache threshold", async () => {
    // Cold cache (12 min) with high history -> should recommend COMPACT
    const planCold = await planInWindowContext({
      baselineTokens: 25_000,
      historyTokens: 90_000,
      remainingTurns: 4,
      apiCallsPerTurn: 6,
      growthPerTurn: 8_000,
      outputPerTurn: 1_000,
      elapsedSecondsSinceLastTurn: 720, // 12 min
    });
    expect(planCold.recommendedAction).toBe("COMPACT");
    expect(planCold.estimatedSavingPercentage).toBeGreaterThanOrEqual(0.15);
    expect(planCold.warmNow).toBe(false);

    // Hot cache (1 min) with low history -> should recommend CONTINUE
    const planHot = await planInWindowContext({
      baselineTokens: 25_000,
      historyTokens: 15_000,
      remainingTurns: 2,
      apiCallsPerTurn: 6,
      growthPerTurn: 8_000,
      outputPerTurn: 1_000,
      elapsedSecondsSinceLastTurn: 60, // 1 min
    });
    expect(planHot.recommendedAction).toBe("CONTINUE");
    expect(planHot.warmNow).toBe(true);
  });

  it("evaluates window reuse (mock disables outside Tauri)", async () => {
    const res = await evaluateWindowReuse({
      modelA: "claude-3-5-sonnet",
      modelB: "claude-3-5-sonnet",
      roleA: "EXECUTOR",
      roleB: "EXECUTOR",
      domainA: "frontend",
      domainB: "frontend",
      writeScopeA: ["src/components/"],
      writeScopeB: ["src/components/"],
      historyTokensA: 15_000,
      baselineTokens: 25_000,
      briefTokensB: 2_000,
      expectedTurnsB: 2,
      apiCallsPerTurn: 4,
      growthPerTurn: 4_000,
      outputPerTurn: 1_000,
      elapsedSeconds: 60,
    });
    // Production path hits decide_window_reuse; vitest uses the safe mock.
    expect(res.canReuse).toBe(false);
    expect(res.reason).toMatch(/Mock|reuse/i);
  });

  it("routes appended messages via triage", async () => {
    const route = await triageMainSessionMessage({
      message: "请问这个概念是什么？",
      activeTasks: [],
      currentTimeMs: 1000,
    });
    expect(route.type).toBe("DIRECT_DISPATCH");
  });

  it("builds compact payload with retention instruction", async () => {
    const payload = await buildCompact({
      providerName: "Claude",
      taskGoal: "Fix unit test failures",
      contractPaths: ["contracts/auth.md"],
    });
    expect(payload.instruction).toContain("保留当前任务目标");
  });
});
