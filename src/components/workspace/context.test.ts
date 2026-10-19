import { describe, it, expect } from "vitest";
import {
  evaluateWindowReuse,
  triageMainSessionMessage,
} from "../../lib/ipc";

describe("Frontend Context Lifecycle Engine (P8.m1)", () => {

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

});
