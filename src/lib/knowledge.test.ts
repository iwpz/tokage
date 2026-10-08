import { describe, it, expect, vi, beforeEach } from "vitest";
import { parsePack, serializePack, PackError } from "./knowledge";
import { useKnowledge } from "../stores/knowledge";
import * as ipc from "./ipc";
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

  it("loads built-in pack offline via bundled JSON import (P6.m1b / F17.c)", () => {
    const { pack } = parsePack(builtinPackText);
    expect(pack.packVersion).toBe("builtin-0.0.1");
    expect(pack.models.length).toBeGreaterThan(0);
    expect(Object.keys(pack.providers).length).toBeGreaterThan(0);
  });
});

describe("custom knowledge pack management & export (P6.m3)", () => {
  it("serializes and exports a compliant KnowledgePack (P6.m3c)", () => {
    const { pack } = parsePack(builtinPackText);
    const jsonStr = serializePack(pack);
    const reParsed = parsePack(jsonStr);
    expect(reParsed.pack.schemaVersion).toBe(1);
    expect(reParsed.pack.packVersion).toBe(pack.packVersion);
    expect(reParsed.pack.models.length).toBe(pack.models.length);
    expect(Object.keys(reParsed.pack.providers).length).toBe(Object.keys(pack.providers).length);
  });

  it("handles invalid JSON or invalid schema with clear error and without corrupting existing packs (P6.m3 acceptance)", () => {
    const invalidJson = "{ malformed json ";
    expect(() => parsePack(invalidJson)).toThrow(PackError);

    const invalidSchema = JSON.stringify({
      schema_version: 999,
      pack_version: "invalid",
      models: {},
    });
    expect(() => parsePack(invalidSchema)).toThrow(/Unsupported schema_version/);

    const invalidModels = JSON.stringify({
      schema_version: 1,
      pack_version: "invalid",
      models: "not-an-object",
    });
    expect(() => parsePack(invalidModels)).toThrow(/models.*must be an object/);
  });

  it("manages multi-pack ordering, local file import, and model overrides in useKnowledge store (P6.m3a, P6.m3b, P6.m3c)", () => {
    useKnowledge.getState().resetToBuiltin();

    // Verify initial state
    expect(useKnowledge.getState().customPacks).toHaveLength(0);
    expect(useKnowledge.getState().custom).toBeNull();

    // 1. Local file import
    const validCustomJson = JSON.stringify({
      schema_version: 1,
      pack_version: "custom-local-1.0",
      domains: ["testing"],
      models: {
        "custom-model": {
          display_name: "Custom Model",
          skills: { testing: 4 },
          price_per_mtok: { input: 2, output: 8 },
          context_window: 64000,
        },
      },
      providers: {
        P_TEST: {
          billing: "METERED",
          models: [{ model: "custom-model", cli_model_arg: "cm" }],
        },
      },
    });

    useKnowledge.getState().importFromFile("my-pack.json", validCustomJson);
    const packsAfterImport = useKnowledge.getState().customPacks;
    expect(packsAfterImport).toHaveLength(1);
    expect(packsAfterImport[0].name).toBe("my-pack.json");
    expect(packsAfterImport[0].isLocalFile).toBe(true);
    expect(packsAfterImport[0].enabled).toBe(true);
    expect(useKnowledge.getState().custom?.pack.packVersion).toBe("custom-local-1.0");

    // 2. Importing invalid JSON throws error without modifying active pack (Acceptance)
    expect(() => {
      useKnowledge.getState().importFromFile("bad.json", "{ invalid");
    }).toThrow();
    expect(useKnowledge.getState().customPacks).toHaveLength(1);
    expect(useKnowledge.getState().custom?.pack.packVersion).toBe("custom-local-1.0");

    // 3. Multi-pack toggle and reordering
    const secondCustomJson = JSON.stringify({
      schema_version: 1,
      pack_version: "custom-2.0",
      models: {
        "m-2": {
          display_name: "Model Two",
          skills: { testing: 3 },
          price_per_mtok: { input: 1, output: 2 },
          context_window: 32000,
        },
      },
      providers: {},
    });
    useKnowledge.getState().importFromFile("second.json", secondCustomJson);
    expect(useKnowledge.getState().customPacks).toHaveLength(2);
    const [second, first] = useKnowledge.getState().customPacks;
    expect(second.name).toBe("second.json");
    expect(first.name).toBe("my-pack.json");

    // Move first up
    useKnowledge.getState().moveCustomPack(first.id, "up");
    expect(useKnowledge.getState().customPacks[0].name).toBe("my-pack.json");

    // Toggle pack
    useKnowledge.getState().toggleCustomPack(first.id);
    expect(useKnowledge.getState().customPacks[0].enabled).toBe(false);

    // 4. Model overrides (P6.m3c)
    const { pack } = parsePack(builtinPackText);
    const sampleModel = pack.models[0];
    useKnowledge.getState().addModelOverride(sampleModel);
    expect(useKnowledge.getState().modelOverrides[sampleModel.modelId]).toBeDefined();
    expect(useKnowledge.getState().modelOverrides[sampleModel.modelId].displayName).toBe(sampleModel.displayName);

    useKnowledge.getState().removeModelOverride(sampleModel.modelId);
    expect(useKnowledge.getState().modelOverrides[sampleModel.modelId]).toBeUndefined();
  });
});

describe("F8: knowledge update channel gate", () => {
  beforeEach(() => {
    useKnowledge.setState({
      checkingUpdate: false,
      updateMessage: null,
      error: null,
      stagedUpdate: null,
    });
  });

  it("F8.a: checkOfficialUpdate skips IPC when knowledgeUpdateEnabled is false", async () => {
    const buildSpy = vi.spyOn(ipc, "getBuildInfo").mockResolvedValue({
      version: "0.0.0",
      commit: null,
      kind: "local",
      repoUrl: ipc.REPO_URL,
      sourceUrl: `${ipc.REPO_URL}/tree/master`,
      knowledgeUpdateEnabled: false,
    });
    const checkSpy = vi.spyOn(ipc, "checkKnowledgePackUpdate").mockResolvedValue(null);

    await useKnowledge.getState().checkOfficialUpdate("2026.01.01");

    expect(buildSpy).toHaveBeenCalled();
    expect(checkSpy).not.toHaveBeenCalled();
    expect(useKnowledge.getState().checkingUpdate).toBe(false);

    buildSpy.mockRestore();
    checkSpy.mockRestore();
  });
});

