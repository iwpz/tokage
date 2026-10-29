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
    expect(next.settingsVersion).toBe(SETTINGS_VERSION);
  });

  it("v4: folds legacy bans into modelChannelAccess", () => {
    const next = migrateSettings({
      settingsVersion: 3,
      isolation: "inplace",
      knowledgeAutoUpdate: true,
      modelBanningLevel: "strict",
      blockedModels: ["Claude Fable 5.1"],
      modelProviderPreferences: {
        opus: { prefer: "CLAUDE_CODE", never: ["CURSOR_CLI"] },
      },
    });
    expect(next.settingsVersion).toBe(SETTINGS_VERSION);
    expect(next.isolation).toBe("inplace");
    expect(next.modelBanningLevel).toBe("off");
    expect(next.blockedModels).toEqual([]);
    expect(next.modelChannelAccess?.opus?.prefer).toBe("CLAUDE_CODE");
    expect(next.modelChannelAccess?.opus?.channels.CURSOR_CLI).toBe(false);
    expect(next.modelChannelAccess?.fable?.denyAll).toBe(true);
    expect(next.modelChannelAccess?.sol?.denyAll).toBe(true);
    expect(next.contextMode).toBe("off");
  });

  it("v5: turns off context suggestion mode", () => {
    const next = migrateSettings({
      settingsVersion: 4,
      isolation: "inplace",
      knowledgeAutoUpdate: true,
      contextMode: "suggest",
      modelChannelAccess: { opus: { prefer: "CLAUDE_CODE", channels: {} } },
    });
    expect(next.isolation).toBe("inplace");
    expect(next.knowledgeAutoUpdate).toBe(true);
    expect(next.settingsVersion).toBe(SETTINGS_VERSION);
    expect(next.contextMode).toBe("off");
    expect(next.modelChannelAccess?.opus?.prefer).toBe("CLAUDE_CODE");
  });

  it("preserves other version 5 choices but maps historical manual-only auto to suggest", () => {
    const next = migrateSettings({
      settingsVersion: 5,
      isolation: "inplace",
      knowledgeAutoUpdate: true,
      contextMode: "auto",
      modelChannelAccess: { opus: { prefer: "CLAUDE_CODE", channels: {} } },
    });
    expect(next.isolation).toBe("inplace");
    expect(next.knowledgeAutoUpdate).toBe(true);
    expect(next.settingsVersion).toBe(SETTINGS_VERSION);
    expect(next.contextMode).toBe("suggest");
    expect(next.modelChannelAccess?.opus?.prefer).toBe("CLAUDE_CODE");
  });

  it("F11.i: defaults isolation to worktree", () => {
    expect(useSettings.getState().isolation).toBe("worktree");
    expect(useSettings.getState().settingsVersion).toBe(SETTINGS_VERSION);
  });

  it("F8.b: knowledgeAutoUpdate defaults to false", () => {
    expect(useSettings.getState().knowledgeAutoUpdate).toBe(false);
  });

  it("tokageCoAuthor defaults to false and can be toggled", () => {
    expect(useSettings.getState().tokageCoAuthor).toBe(false);
    useSettings.getState().set("tokageCoAuthor", true);
    expect(useSettings.getState().tokageCoAuthor).toBe(true);
    useSettings.getState().set("tokageCoAuthor", false);
  });

  it("defaults include modelChannelAccess with opus prefer", () => {
    expect(useSettings.getState().modelChannelAccess.opus?.prefer).toBe("CLAUDE_CODE");
  });

  it("F3.c: agent hard timeout is raised when idle exceeds it", () => {
    useSettings.setState({ agentIdleTimeoutMin: 30, agentHardTimeoutMin: 120 });
    useSettings.getState().set("agentIdleTimeoutMin", 150);
    expect(useSettings.getState().agentIdleTimeoutMin).toBe(150);
    expect(useSettings.getState().agentHardTimeoutMin).toBe(150);
  });
});

it("never imports retired or native usage consent from any settings version", () => {
  for (const settingsVersion of [0, SETTINGS_VERSION, SETTINGS_VERSION + 1]) {
    const imported = { settingsVersion, activeQuotaQuery: true, allowOnlineUsageQuery: true };
    const migrated = migrateSettings(imported);
    expect(migrated).not.toHaveProperty("activeQuotaQuery");
    expect(migrated).not.toHaveProperty("allowOnlineUsageQuery");
    expect(imported.activeQuotaQuery).toBe(true);
  }
});

it("M2.a keeps default off, migrates old manual-only auto, and preserves explicit new auto",()=>{
 expect(useSettings.getState().contextMode).toBe("off");
 for(const contextMode of ["off","suggest","auto"] as const) {
  expect(migrateSettings({settingsVersion:6,contextMode}).contextMode).toBe(contextMode==="auto"?"suggest":contextMode);
  expect(migrateSettings({settingsVersion:SETTINGS_VERSION,contextMode}).contextMode).toBe(contextMode);
 }
});

it("extra context report calls require explicit opt-in and preserve it during migration",()=>{
 expect(useSettings.getState().contextGenerateReport).toBe(false);
 expect(migrateSettings({settingsVersion:6})).not.toHaveProperty("contextGenerateReport",true);
 expect(migrateSettings({settingsVersion:SETTINGS_VERSION,contextGenerateReport:true}).contextGenerateReport).toBe(true);
});
