import { describe, it, expect } from "vitest";
import { parsePack, PackError } from "./knowledge";
import builtinPackText from "../../src-tauri/resources/knowledge/builtin.json?raw";

describe("knowledge pack parser (P6.m1a)", () => {
  it("parses the complete builtin knowledge pack cleanly", () => {
    const { pack, warnings } = parsePack(builtinPackText);
    expect(warnings).toEqual([]);
    expect(pack.schemaVersion).toBe(1);
    expect(pack.packVersion).toContain("builtin");
    expect(pack.domains.length).toBeGreaterThan(0);
    expect(pack.models.length).toBeGreaterThan(0);
    expect(Object.keys(pack.providers).length).toBeGreaterThan(0);
    expect(pack.successModel).not.toBeNull();
    expect(pack.successModel?.a).toBe(1.6);
    expect(pack.successModel?.b).toBe(1.2);
    expect(pack.successModel?.priorStrength).toBe(6);
    expect(pack.successModel?.req["1"]?.["S"]).toBe(2.0);
    expect(pack.successModel?.req["3"]?.["L"]).toBe(4.5);
    expect(pack.successModel?.plannerMinSkill).toBe(5);

    // Verify provider details
    const claudeProvider = pack.providers["CLAUDE_CODE"];
    expect(claudeProvider).toBeDefined();
    expect(claudeProvider.billing).toBe("SUBSCRIPTION");
    expect(claudeProvider.cache?.mode).toBe("explicit_prefix");
    expect(claudeProvider.cache?.ttlSeconds).toBe(300);
    expect(claudeProvider.context?.compact).toBe("native");
    expect(claudeProvider.models.length).toBeGreaterThan(0);

    // Verify model details & CostModel parameters
    const opus = pack.models.find((m) => m.modelId === "claude-opus-5-5");
    expect(opus).toBeDefined();
    expect(opus?.price).toEqual({ input: 4, output: 20 });
    expect(opus?.cacheRates).toEqual({ readMultiplier: 0.05, writeMultiplier: 1.25 });
    expect(opus?.maxOutput).toBe(128000);
    expect(opus?.offerings.length).toBeGreaterThan(0);

    // Verify usage profiles
    expect(pack.usageProfiles["planning"]?.["S"]).toBeDefined();
    expect(pack.usageProfiles["frontend-ui"]?.["M"]).toBeDefined();
    expect(pack.usageProfiles["frontend-ui"]?.["M"].growthPerTurn).toBe(18000);
  });

  it("throws PackError on unsupported schema version", () => {
    const bad = JSON.stringify({ schema_version: 99, pack_version: "bad", models: {} });
    expect(() => parsePack(bad)).toThrow(PackError);
  });

  it("records warnings for clamped skills and missing models in offerings", () => {
    const sample = JSON.stringify({
      schema_version: 1,
      pack_version: "custom-1.0",
      domains: ["frontend-ui"],
      models: {
        "m-1": {
          display_name: "Model 1",
          skills: { "frontend-ui": 10, "unknown-domain": 3 },
          price_per_mtok: { input: 1, output: 2 },
          context_window: 100000,
          effective_context: 50000,
        },
      },
      providers: {
        P1: {
          billing: "METERED",
          models: [{ model: "m-1" }, { model: "m-unknown" }],
        },
      },
    });

    const { pack, warnings } = parsePack(sample);
    expect(pack.models[0].skills["frontend-ui"]).toBe(5); // Clamped
    expect(warnings.some((w) => w.includes("clamped to 1–5"))).toBe(true);
    expect(warnings.some((w) => w.includes("unknown domain"))).toBe(true);
    expect(warnings.some((w) => w.includes('unknown model "m-unknown"'))).toBe(true);
  });

  it("loads built-in pack offline via getBuiltinKnowledgePack (P6.m1b)", async () => {
    const { getBuiltinKnowledgePack } = await import("./ipc");
    const json = await getBuiltinKnowledgePack();
    const { pack } = parsePack(json);
    expect(pack.packVersion).toBe("builtin-0.0.1");
    expect(pack.models.length).toBeGreaterThan(0);
    expect(Object.keys(pack.providers).length).toBeGreaterThan(0);
  });
});
