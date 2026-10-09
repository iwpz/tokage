import { describe, expect, it, beforeEach } from "vitest";
import { fixedModelOverrides, isFixedMode, modelSpecFromRef } from "./fixedModel";
import type { Plan } from "./ipc";
import { useChat } from "../stores/chat";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";

function emptyPlan(taskIds: string[]): Plan {
  return {
    spec: {
      raw: "",
      goal: "g",
      inScope: [],
      outOfScope: [],
      decisions: [],
    },
    contracts: [],
    tasks: taskIds.map((id) => ({
      id,
      title: id,
      goal: "",
      domain: "general",
      difficulty: 1,
      size: "S" as const,
      depends_on: [],
      contracts: [],
      write_scope: [],
      deliverables: [],
      assigned_model: null,
      acceptance: { commands: [], criteria: [] },
    })),
    rawMarkdown: "",
  };
}

describe("fixedModel", () => {
  beforeEach(() => {
    useChat.setState({
      conversations: {},
      picks: { plan: null, direct: null, fixed: null },
      modelOptions: {},
    });
    useSettings.setState({
      providerEnabled: {},
      providerTrust: {},
      blockedModels: [],
      modelBanningLevel: "off",
      modelProviderPreferences: {},
      modelChannelAccess: {},
    });
    useProviders.setState({ providers: [] });
  });

  it("isFixedMode recognizes fixed", () => {
    expect(isFixedMode("fixed")).toBe(true);
    expect(isFixedMode("auto")).toBe(false);
    expect(isFixedMode("plan")).toBe(false);
    expect(isFixedMode("direct")).toBe(false);
  });

  it("fixedModelOverrides pins every task to the same spec", () => {
    const plan = emptyPlan(["T1", "T2", "T3"]);
    const overrides = fixedModelOverrides(plan, {
      provider: "CLAUDE_CODE",
      model: "sonnet",
    });
    expect(overrides).toEqual({
      T1: "CLAUDE_CODE:sonnet",
      T2: "CLAUDE_CODE:sonnet",
      T3: "CLAUDE_CODE:sonnet",
    });
  });

  it("modelSpecFromRef falls back when catalog is empty", () => {
    expect(modelSpecFromRef({ provider: "CODEX_CLI", model: "gpt-5" })).toBe("CODEX_CLI:gpt-5");
  });
});
