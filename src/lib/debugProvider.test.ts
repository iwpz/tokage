import { beforeEach, describe, expect, it, vi } from "vitest";
const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: call }));
import { useSettings, migrateSettings } from "../stores/settings";
import { DEFAULT_DEBUG_PROVIDER } from "./debugProviderTypes";
import { builtinDebugCatalog, debugAttempt, debugCatalogOverlay } from "./debugProvider";
import { createSessionWorktree, executeGateCommand, recordTaskOutcome, startChatTurn, type AttemptRow, type ChatRequest, type TaskOutcomeRow } from "./ipc";
import { selectableGroups } from "./models";

const request = (id: string): ChatRequest => ({ turnId: id, cwd: `/original-${id}`, prompt: "a request", mode: "plan", provider: "CLAUDE_CODE", model: "opus", effort: "high", fast: true, tier: null, history: [], resume: null, trusted: [] });

beforeEach(() => {
  useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER } });
  call.mockReset();
  call.mockImplementation((name: string, args: any) => Promise.resolve(name === "prepare_debug_workspace" ? `/fixture${args.path}` : undefined));
});

describe("Debug Provider", () => {
  it("migrates both old surrogate choices and preserves explicit new settings", () => {
    expect(migrateSettings({ settingsVersion: 5, devShadowModel: "luna" }).debugProvider?.output).toBe("luna");
    expect(migrateSettings({ settingsVersion: 5, devShadowModel: "deepseek" }).debugProvider?.output).toBe("deepseek");
    expect(migrateSettings({ settingsVersion: 5, devShadowModel: "none" }).debugProvider).toEqual(DEFAULT_DEBUG_PROVIDER);
    expect(migrateSettings({ settingsVersion: 5, devShadowModel: "luna", debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "mock" } }).debugProvider?.output).toBe("mock");
  });

  it("provides the complete bundled model catalog without installing or trusting simulated CLIs", () => {
    const catalog = builtinDebugCatalog();
    expect(catalog.length).toBeGreaterThan(5);
    const groups = selectableGroups(catalog, {}, {});
    expect(groups.length).toBe(catalog.length);
    expect(groups.some((g) => g.provider.id === "CLAUDE_CODE" && g.models.some((m) => m.key === "opus"))).toBe(true);
    const real = [{ ...catalog[0], debugVirtual: false, status: "NOT_INSTALLED" as const }];
    expect(debugCatalogOverlay(real, catalog, DEFAULT_DEBUG_PROVIDER)).toBe(real);
    expect(debugCatalogOverlay(real, catalog, { ...DEFAULT_DEBUG_PROVIDER, output: "mock" }).every((p) => p.debugVirtual)).toBe(true);
  });

  it("routes all turn roles centrally while preserving logical model/parameters", async () => {
    for (const role of ["PLANNER", "EXECUTOR", "TESTER", "REVIEWER"] as const) {
      useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "luna" } });
      await startChatTurn({ ...request(role), role });
      const args = call.mock.calls.at(-1)![1];
      expect(args.request).toMatchObject({ provider: "CLAUDE_CODE", model: "opus", effort: "high", fast: true, debug: { output: "luna", lunaProvider: "CODEX_CLI", lunaModel: "gpt-6-luna" } });
      expect(call.mock.calls.at(-1)![0]).toBe("start_chat_turn");
    }
  });

  it("isolates mock turns, worktree creation, and gates at the shared IPC boundary", async () => {
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "mock", scenario: "repair" } });
    await startChatTurn(request("isolation"));
    expect(call.mock.calls.at(-1)![1].request.cwd).toBe("/fixture/original-isolation");
    await createSessionWorktree("/new-original", "task");
    expect(call.mock.calls.at(-1)).toEqual(["create_session_worktree", { path: "/fixture/new-original", name: "task", baseRef: null }]);
    await executeGateCommand("npm test", "/gate-original", undefined, false, undefined, "gate:s:T1");
    expect(call.mock.calls.at(-1)![1]).toMatchObject({ workdir: "/fixture/gate-original", debugConfig: { output: "mock", scenario: "repair" } });
  });

  it("attributes an attempt to its surrogate and keeps that provenance after settings change", async () => {
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "deepseek", deepseekProvider: "OPENCODE", deepseekModel: "deepseek/custom" } });
    await startChatTurn(request("provenance"));
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER } });
    const row = { id: "provenance", mainSessionId: "s", providerName: "CLAUDE_CODE", modelId: "opus", execMode: "CLI" } as AttemptRow;
    expect(debugAttempt(row)).toMatchObject({ execMode: "HEADLESS", debugOutput: "deepseek", requestedProvider: "CLAUDE_CODE", requestedModel: "opus", providerName: "OPENCODE", modelId: "deepseek/custom" });
    const outcome = { modelId: "opus" } as TaskOutcomeRow;
    call.mockClear();
    expect(await recordTaskOutcome(outcome, "provenance")).toBe(0);
    expect(call).not.toHaveBeenCalled();
  });

  it("never sends the selected expensive model to a desktop backend without debug support", async () => {
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "luna" } });
    call.mockRejectedValue(new Error("unknown command"));
    await expect(startChatTurn(request("old-backend"))).rejects.toThrow();
    expect(call.mock.calls.map(([name]) => name)).toEqual(["get_debug_provider_catalog"]);
  });
});

it("pins persisted demo turns to Mock even after debug mode is disabled or switched to a paid source", async () => {
  for (const output of ["off", "luna", "deepseek"] as const) {
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output } });
    const demo = { ...request(`demo-${output}`), sessionId: `tokage-demo-success-zh-${output}` };
    await startChatTurn(demo);
    expect(call.mock.calls.at(-1)).toEqual(["start_chat_turn", { request: expect.objectContaining({
      provider: demo.provider, model: demo.model, debug: expect.objectContaining({ output: "mock", demo: true, demoLanguage: "zh" }),
    }) }]);
  }
  expect(call.mock.calls.some(([name]) => name === "get_debug_provider_catalog")).toBe(false);
});
