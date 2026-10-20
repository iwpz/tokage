// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, createElement, Profiler } from "react";
import { createRoot, type Root } from "react-dom/client";

const api = vi.hoisted(() => ({ attemptReads: 0, logReads: 0 }));
vi.mock("../../lib/ipc", async (original) => ({
  ...await original<object>(),
  listAttempts: vi.fn(async () => { api.attemptReads++; return [{ id: "a", providerName: "CLAUDE_CODE", modelId: "test", role: "USER", turnCount: 1, status: "DONE", contextTokens: 0, createdAt: "2025-01-01T00:00:00Z" }]; }),
  getChatLogs: vi.fn(async () => { api.logReads++; return [{ id: 1, messageId: "m", mainSessionId: "s", attemptId: "a", seq: 0, turnIndex: 0, role: "ASSISTANT", content: "hello", contentTruncated: false, metaJson: "{}", createdAt: "2025-01-01T00:00:00Z" }]; }),
}));

import { SessionHistoryView } from "./SessionHistoryView";

describe("SessionHistoryView ordered snapshots", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    vi.useFakeTimers();
    (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
    api.attemptReads = 0; api.logReads = 0;
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

  it("keeps rendered commit count flat through 100 real equal poll cycles while reads continue", async () => {
    let commits = 0;
    await act(async () => {
      root.render(createElement(Profiler, { id: "history", onRender: () => { commits++; } }, createElement(SessionHistoryView, { sessionId: "s" })));
      for (let i = 0; i < 8; i++) await Promise.resolve();
    });
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(host.textContent).toContain("hello");
    const settledCommits = commits;
    const attemptReads = api.attemptReads;
    const logReads = api.logReads;
    for (let i = 0; i < 100; i++) {
      await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    }
    expect(commits).toBe(settledCommits);
    expect(api.attemptReads).toBeGreaterThanOrEqual(attemptReads + 100);
    expect(api.logReads).toBeGreaterThanOrEqual(logReads + 100);
  });
});
