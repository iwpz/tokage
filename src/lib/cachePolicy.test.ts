import { describe, it, expect } from "vitest";
import {
  resolveModelCachePolicy,
  calculateSessionCacheStatus,
} from "./cachePolicy";

describe("cachePolicy", () => {
  it("resolves Claude models as ephemeral 5-minute TTL", () => {
    const policy = resolveModelCachePolicy("claude-3-5-sonnet", "CLAUDE_CODE");
    expect(policy.cacheType).toBe("ephemeral");
    expect(policy.ttlSeconds).toBe(300);
  });

  it("resolves DeepSeek models as disk_persistent", () => {
    const policy = resolveModelCachePolicy("deepseek-chat");
    expect(policy.cacheType).toBe("disk_persistent");
    expect(policy.ttlSeconds).toBe(86400);
  });

  it("resolves OpenAI / Codex models as ephemeral 30-minute TTL", () => {
    const policy = resolveModelCachePolicy("gpt-4o", "CODEX_CLI");
    expect(policy.cacheType).toBe("ephemeral");
    expect(policy.ttlSeconds).toBe(1800);
  });

  it("resolves Gemini / Antigravity as implicit", () => {
    const policy = resolveModelCachePolicy("gemini-2.5-pro", "ANTIGRAVITY_CLI");
    expect(policy.cacheType).toBe("implicit");
    expect(policy.ttlSeconds).toBe(0);
  });

  it("resolves models without cache support as none", () => {
    const policy = resolveModelCachePolicy(undefined, undefined);
    expect(policy.cacheType).toBe("none");
  });

  it("calculates session cache status correctly across elapsed intervals", () => {
    const now = 1_000_000;

    // Claude: 300s TTL * 0.9 = 270s effective threshold
    // 60s elapsed -> warm (210s remaining)
    const claudeWarm = calculateSessionCacheStatus(now - 60_000, "claude-3-5-sonnet", "CLAUDE_CODE", now);
    expect(claudeWarm.isWarm).toBe(true);
    expect(claudeWarm.ttlRemainingSeconds).toBe(210);

    // 280s elapsed -> cold (0s remaining)
    const claudeCold = calculateSessionCacheStatus(now - 280_000, "claude-3-5-sonnet", "CLAUDE_CODE", now);
    expect(claudeCold.isWarm).toBe(false);
    expect(claudeCold.ttlRemainingSeconds).toBe(0);

    // DeepSeek: 10 minutes elapsed -> still warm (disk_persistent)
    const deepseekStatus = calculateSessionCacheStatus(now - 600_000, "deepseek-coder", undefined, now);
    expect(deepseekStatus.isWarm).toBe(true);
    expect(deepseekStatus.cacheType).toBe("disk_persistent");

    // No completed turn (undefined endedAt) -> cold / none
    const emptyStatus = calculateSessionCacheStatus(undefined, "claude-3-5-sonnet", "CLAUDE_CODE", now);
    expect(emptyStatus.isWarm).toBe(false);
    expect(emptyStatus.ttlRemainingSeconds).toBe(0);
  });
});

it("normalizes planner spellings and keeps absent, implicit and expired caches cold", async()=>{
 const {canonicalCacheType,contextCacheWarmth}=await import("./cachePolicy");
 for(const mode of ["standard","ephemeral","explicit_prefix","automatic_prefix"]) expect(canonicalCacheType(mode)).toBe("ephemeral");
 expect(contextCacheWarmth(269,300,"ephemeral").isWarm).toBe(true);
 for(const mode of ["ephemeral","disk_persistent"]) expect(contextCacheWarmth(270,300,mode)).toEqual({isWarm:false,ttlRemainingSeconds:0});
 expect(contextCacheWarmth(0,300,"none").isWarm).toBe(false);
 expect(contextCacheWarmth(0,300,"unknown").isWarm).toBe(false);
 expect(contextCacheWarmth(0,300,"implicit",0).isWarm).toBe(false);
});
