import { describe, expect, it } from "vitest";
import {
  addCustomFamilyFromCatalog,
  banUltraFamilies,
  defaultModelChannelAccess,
  familyOf,
  isChannelDenied,
  isFamilyAllowedOnChannel,
  matrixFamilyRows,
  migrateToModelChannelAccess,
  providerOffersModel,
  removeCustomFamily,
  setFamilyChannel,
} from "./modelAccess";
import { isModelBlocked, selectableGroups } from "./models";

describe("familyOf", () => {
  it("resolves ultra families before broader gpt-6", () => {
    expect(familyOf("claude-fable-5-1")).toBe("fable");
    expect(familyOf("gpt-6-astra")).toBe("astra");
    expect(familyOf("claude-opus-5-5")).toBe("opus");
    expect(familyOf("gpt-6.1-sol")).toBe("sol");
    expect(familyOf("gpt-6-luna")).toBe("gpt-6");
    expect(familyOf("deepseek/deepseek-v4-pro")).toBe("deepseek");
    expect(familyOf("claude-sonnet-5-5")).toBe("sonnet");
  });
});

describe("modelChannelAccess matrix", () => {
  it("denies a family on one channel while allowing another", () => {
    const access = {
      opus: { prefer: "CLAUDE_CODE", channels: { CURSOR_CLI: false } },
    };
    expect(isChannelDenied("claude-opus-5-5", "CURSOR_CLI", access)).toBe(true);
    expect(isChannelDenied("claude-opus-5-5", "CLAUDE_CODE", access)).toBe(false);
    expect(isFamilyAllowedOnChannel(access, "opus", "CURSOR_CLI")).toBe(false);
    expect(isFamilyAllowedOnChannel(access, "opus", "CLAUDE_CODE")).toBe(true);
  });

  it("denyAll blocks every channel", () => {
    const access = banUltraFamilies({});
    expect(isChannelDenied("claude-fable-5-1", "CLAUDE_CODE", access)).toBe(true);
    expect(isChannelDenied("gpt-6-astra", "CODEX_CLI", access)).toBe(true);
    expect(isChannelDenied("gpt-6.1-sol", "COPILOT_CLI", access)).toBe(true);
  });

  it("setFamilyChannel lifts denyAll and keeps other known channels denied", () => {
    let access = banUltraFamilies({});
    access = setFamilyChannel(access, "opus", "CLAUDE_CODE", true, [
      "CLAUDE_CODE",
      "CURSOR_CLI",
      "COPILOT_CLI",
    ]);
    expect(access.opus.denyAll).toBe(false);
    expect(isFamilyAllowedOnChannel(access, "opus", "CLAUDE_CODE")).toBe(true);
    expect(isFamilyAllowedOnChannel(access, "opus", "CURSOR_CLI")).toBe(false);
  });
});

describe("migrateToModelChannelAccess", () => {
  it("folds prefs.never, banning level, and blockedModels", () => {
    const access = migrateToModelChannelAccess({
      modelProviderPreferences: {
        opus: { prefer: "CLAUDE_CODE", never: ["CURSOR_CLI", "COPILOT_CLI"] },
      },
      modelBanningLevel: "moderate",
      blockedModels: ["Claude Fable 5.1", "Codex CLI · GPT 6 Astra"],
    });
    expect(access.opus?.prefer).toBe("CLAUDE_CODE");
    expect(access.opus?.channels.CURSOR_CLI).toBe(false);
    expect(access.fable?.denyAll).toBe(true);
    expect(access.astra?.denyAll).toBe(true);
  });

  it("keeps default opus prefer/never shape", () => {
    const access = defaultModelChannelAccess();
    expect(access.opus.prefer).toBe("CLAUDE_CODE");
    expect(access.opus.channels.CURSOR_CLI).toBe(false);
  });
});

describe("custom matrix rows from catalog", () => {
  it("adds a custom row and matches via familyOf", () => {
    const result = addCustomFamilyFromCatalog({}, "claude-haiku-4-5", "Claude Haiku 4.5");
    expect(result?.alreadyPresent).toBe(false);
    expect(result?.key).toBe("claude-haiku-4-5");
    expect(familyOf("claude-haiku-4-5", result?.access)).toBe("claude-haiku-4-5");
    expect(matrixFamilyRows(result?.access).some((r) => r.custom && r.key === "claude-haiku-4-5")).toBe(
      true,
    );
  });

  it("does not duplicate builtin families", () => {
    const result = addCustomFamilyFromCatalog({}, "claude-opus-5-5", "Claude Opus 5.5");
    expect(result?.alreadyPresent).toBe(true);
    expect(result?.key).toBe("opus");
  });

  it("removes custom rows but not builtins", () => {
    let access = addCustomFamilyFromCatalog({}, "composer-1", "Composer")!.access;
    access = removeCustomFamily(access, "composer-1");
    expect(access["composer-1"]).toBeUndefined();
    access = removeCustomFamily({ opus: { channels: {}, prefer: "CLAUDE_CODE" } }, "opus");
    expect(access.opus).toBeDefined();
  });
});

describe("providerOffersModel", () => {
  it("does not claim DeepSeek on Claude Code", () => {
    const claude = {
      models: [{ id: "opus", name: "Opus" }],
      modelGroups: [{ key: "opus", name: "Opus" }],
    };
    const aider = {
      models: [{ id: "deepseek/deepseek-v4-pro", name: "DeepSeek V4 Pro" }],
      modelGroups: [{ key: "deepseek-v4-pro", name: "DeepSeek V4 Pro" }],
    };
    expect(providerOffersModel(claude, "deepseek")).toBe(false);
    expect(providerOffersModel(aider, "deepseek")).toBe(true);
  });
});

describe("isModelBlocked via matrix", () => {
  it("filters selectableGroups by channel matrix", () => {
    const mockProviders = [
      {
        id: "CLAUDE_CODE",
        name: "Claude Code",
        executable: "claude",
        featured: true,
        status: "READY",
        version: "2.1.288",
        path: "/usr/local/bin/claude",
        models: [
          { id: "opus", name: "Opus" },
          { id: "sonnet", name: "Sonnet" },
        ],
        modelGroups: [
          {
            key: "opus",
            name: "Opus",
            efforts: [],
            contexts: [],
            thinking: false,
            fast: false,
            tags: [],
            tiers: [],
            variants: [{ id: "opus", thinking: false, fast: false }],
          },
          {
            key: "sonnet",
            name: "Sonnet",
            efforts: [],
            contexts: [],
            thinking: false,
            fast: false,
            tags: [],
            tiers: [],
            variants: [{ id: "sonnet", thinking: false, fast: false }],
          },
        ],
        trustArgs: [],
        installMethods: [],
        guidance: [],
        chat: true,
      },
      {
        id: "CURSOR_CLI",
        name: "Cursor",
        executable: "cursor",
        featured: true,
        status: "READY",
        version: "1.0",
        path: "/usr/local/bin/cursor",
        models: [{ id: "claude-opus-5-5", name: "Opus" }],
        modelGroups: [
          {
            key: "claude-opus-5-5",
            name: "Opus",
            efforts: [],
            contexts: [],
            thinking: false,
            fast: false,
            tags: [],
            tiers: [],
            variants: [{ id: "claude-opus-5-5", thinking: false, fast: false }],
          },
        ],
        trustArgs: [],
        installMethods: [],
        guidance: [],
        chat: true,
      },
    ] as any;

    const enabled = { CLAUDE_CODE: true, CURSOR_CLI: true };
    const trust = { CLAUDE_CODE: true, CURSOR_CLI: true };
    const access = {
      opus: { channels: { CURSOR_CLI: false }, prefer: "CLAUDE_CODE" },
    };

    expect(isModelBlocked("opus", [], "CURSOR_CLI", "off", undefined, access)).toBe(true);
    expect(isModelBlocked("opus", [], "CLAUDE_CODE", "off", undefined, access)).toBe(false);

    const groups = selectableGroups(
      mockProviders,
      enabled,
      trust,
      "plan",
      [],
      "off",
      undefined,
      access,
    );
    const cursor = groups.find((g) => g.provider.id === "CURSOR_CLI");
    const claude = groups.find((g) => g.provider.id === "CLAUDE_CODE");
    expect(cursor?.models.map((m) => m.key) ?? []).toEqual([]);
    expect(claude?.models.map((m) => m.key)).toEqual(["opus", "sonnet"]);
  });
});
