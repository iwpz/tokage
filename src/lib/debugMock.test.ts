import { beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: vi.fn() }));
import { DEFAULT_DEBUG_PROVIDER } from "./debugProviderTypes";
import { debugMockGate, debugMockPlan } from "./debugMock";
import { useSettings } from "../stores/settings";
import { cancelChatTurn, onChatEnded, onChatEvent, parsePlanningOutput, startChatTurn, type ChatEvent, type ChatRequest } from "./ipc";

beforeEach(() => useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER } }));
const request = (id: string): ChatRequest => ({ turnId: id, cwd: "/preview", prompt: "hello", mode: "direct", provider: "CLAUDE_CODE", model: "opus", effort: null, fast: false, tier: null, history: [], resume: null, trusted: [] });

describe("browser mock lifecycle", () => {
  it("streams Unicode text, has zero usage, and ends exactly once after done", async () => {
    vi.useFakeTimers();
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "mock", chunkDelayMs: 5, mockReply: "中文🦎".repeat(50) } });
    const events: ChatEvent[] = []; const ended: string[] = [];
    const off = await onChatEvent((e) => events.push(e)); const offEnded = await onChatEnded((e) => ended.push(e.turnId));
    try {
      await startChatTurn(request("stream"));
      await vi.runAllTimersAsync();
      expect(events.filter((e) => e.kind === "text").map((e) => e.kind === "text" ? e.text : "").join("")).toBe("中文🦎".repeat(50));
      expect(events.at(-1)).toMatchObject({ kind: "done", apiCalls: 0, inputTokens: 0, outputTokens: 0, costUsd: 0 });
      expect(ended).toEqual(["stream"]);
    } finally { off(); offEnded(); vi.useRealTimers(); }
  });

  it("cancels a stalled stream without emitting subsequent chunks or done", async () => {
    vi.useFakeTimers();
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "mock", scenario: "slow" } });
    const events: ChatEvent[] = []; const ended: boolean[] = [];
    const off = await onChatEvent((e) => events.push(e)); const offEnded = await onChatEnded((e) => ended.push(e.cancelled));
    try {
      await startChatTurn(request("cancel"));
      await vi.advanceTimersByTimeAsync(1000);
      await cancelChatTurn("cancel");
      await vi.advanceTimersByTimeAsync(40_000);
      expect(events).toEqual([]); expect(ended).toEqual([true]);
    } finally { off(); offEnded(); vi.useRealTimers(); }
  });

  it("never pretends browser preview has called a paid surrogate", async () => {
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "luna" } });
    await expect(startChatTurn(request("paid"))).rejects.toThrow();
  });

  it("keeps a valid dependency plan and simulates a repair feedback loop", async () => {
    useSettings.setState({ debugProvider: { ...DEFAULT_DEBUG_PROVIDER, output: "mock", scenario: "repair" } });
    const plan = await parsePlanningOutput({ markdown: debugMockPlan().rawMarkdown! });
    expect(plan.tasks[1].depends_on).toEqual(["T1"]);
    const config = useSettings.getState().debugProvider;
    expect(debugMockGate("npm test", "/fixture", "gate:repair-test:T1", config).passed).toBe(false);
    expect(debugMockGate("npm test", "/fixture", "gate:repair-test:T1", config).passed).toBe(true);
    expect(debugMockGate("npm test", "/fixture", "gate:delivery:repair-test", config).passed).toBe(true);
  });
});
