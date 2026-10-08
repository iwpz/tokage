import { describe, expect, it } from "vitest";
import { migrateSettings, SETTINGS_VERSION, useSettings } from "./settings";

describe("settings migration", () => {
  it("F1.g / F8: missing settingsVersion disables knowledgeAutoUpdate", () => {
    const next = migrateSettings({
      isolation: "inplace",
      knowledgeAutoUpdate: true,
    });
    expect(next.knowledgeAutoUpdate).toBe(false);
    expect(next.settingsVersion).toBe(SETTINGS_VERSION);
  });

  it("F11.i: version < 3 → isolation worktree", () => {
    const next = migrateSettings({
      settingsVersion: 2,
      isolation: "inplace",
      knowledgeAutoUpdate: true,
    });
    expect(next.isolation).toBe("worktree");
    expect(next.knowledgeAutoUpdate).toBe(true);
    expect(next.settingsVersion).toBe(3);
  });

  it("leaves version 3 values unchanged", () => {
    const next = migrateSettings({
      settingsVersion: 3,
      isolation: "inplace",
      knowledgeAutoUpdate: true,
    });
    expect(next.isolation).toBe("inplace");
    expect(next.knowledgeAutoUpdate).toBe(true);
    expect(next.settingsVersion).toBe(3);
  });

  it("F11.i: defaults isolation to worktree", () => {
    expect(useSettings.getState().isolation).toBe("worktree");
    expect(useSettings.getState().settingsVersion).toBe(SETTINGS_VERSION);
  });

  it("F8.b: knowledgeAutoUpdate defaults to false", () => {
    expect(useSettings.getState().knowledgeAutoUpdate).toBe(false);
  });

  it("F3.c: agent hard timeout is raised when idle exceeds it", () => {
    useSettings.setState({ agentIdleTimeoutMin: 30, agentHardTimeoutMin: 120 });
    useSettings.getState().set("agentIdleTimeoutMin", 150);
    expect(useSettings.getState().agentIdleTimeoutMin).toBe(150);
    expect(useSettings.getState().agentHardTimeoutMin).toBe(150);
  });
});
