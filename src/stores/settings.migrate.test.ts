import { describe, expect, it } from "vitest";
import { migrateSettings, SETTINGS_VERSION, useSettings } from "./settings";

describe("F1.g settings migration", () => {
  it("migrates missing settingsVersion + worktree → inplace and disables knowledgeAutoUpdate", () => {
    const next = migrateSettings({
      isolation: "worktree",
      knowledgeAutoUpdate: true,
    });
    expect(next.isolation).toBe("inplace");
    expect(next.knowledgeAutoUpdate).toBe(false);
    expect(next.settingsVersion).toBe(SETTINGS_VERSION);
  });

  it("leaves version 2 values unchanged", () => {
    const next = migrateSettings({
      settingsVersion: 2,
      isolation: "worktree",
      knowledgeAutoUpdate: true,
    });
    expect(next.isolation).toBe("worktree");
    expect(next.knowledgeAutoUpdate).toBe(true);
    expect(next.settingsVersion).toBe(2);
  });

  it("defaults isolation to inplace", () => {
    expect(useSettings.getState().isolation).toBe("inplace");
    expect(useSettings.getState().settingsVersion).toBe(SETTINGS_VERSION);
  });
});
