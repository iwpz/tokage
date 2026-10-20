// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

const api = vi.hoisted(() => ({ failAttempts: false, attemptReads: 0, logReads: 0 }));
vi.mock("../../lib/ipc", async (original) => ({
  ...await original<object>(),
  listAttempts: vi.fn(async () => { api.attemptReads++; if (api.failAttempts) throw new Error("database busy"); return [{ id: "a", providerName: "CLAUDE_CODE", modelId: "test", role: "USER", turnCount: 1, status: "DONE", contextTokens: 0, createdAt: "2025-01-01T00:00:00Z" }]; }),
  getChatLogs: vi.fn(async () => { api.logReads++; return []; }),
}));

import { SessionHistoryView } from "./SessionHistoryView";
import { useTaskTranscripts } from "../../stores/taskTranscripts";

describe("SessionHistoryView polling backoff", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    vi.useFakeTimers();
    (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
    api.failAttempts = false; api.attemptReads = 0; api.logReads = 0;
    useTaskTranscripts.setState({ entries: {} });
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: class { observe() {} unobserve() {} disconnect() {} } });
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

  it("backs off list IPC independently, retains rendered state and retries accessibly", async () => {
    await act(async () => { root.render(createElement(SessionHistoryView, { sessionId: "s" })); await Promise.resolve(); });
    expect(host.querySelector('[data-attempt-id="a"]')).toBeTruthy();
    expect(api.attemptReads).toBe(1); expect(api.logReads).toBe(1);
    const transcript = host.querySelector(".relative.flex-1.overflow-y-auto") as HTMLElement;
    expect(transcript).toBeTruthy();
    Object.defineProperties(transcript, { scrollHeight: { configurable: true, value: 500 }, clientHeight: { configurable: true, value: 100 } });
    transcript.scrollTop = 20;
    await act(async () => transcript.dispatchEvent(new Event("scroll")));
    await act(async () => useTaskTranscripts.setState({ entries: { a: {
      request: { sessionId: "s", turnId: "a" } as never,
      messages: [{ id: "live", role: "assistant", text: "live store text during backoff" }] as never,
    } } }));

    api.failAttempts = true;
    await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
    expect(api.attemptReads).toBe(2); expect(api.logReads).toBe(2);
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("Could not load the session list.");
    expect(host.querySelector('[data-attempt-id="a"]')).toBeTruthy();
    expect(host.textContent).toContain("live store text during backoff");
    expect(transcript.scrollTop).toBe(20);

    await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
    expect(api.attemptReads).toBe(3); expect(api.logReads).toBe(3);
    await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
    expect(api.attemptReads).toBe(3); expect(api.logReads).toBe(4);
    const retry = [...host.querySelectorAll("button")].find((button) => button.textContent?.includes("Retry"));
    expect(retry).toBeTruthy();
    api.failAttempts = false;
    await act(async () => { retry!.click(); await Promise.resolve(); });
    expect(api.attemptReads).toBe(4);
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector('[data-attempt-id="a"]')).toBeTruthy();
    await act(async () => { await vi.advanceTimersByTimeAsync(999); });
    expect(api.attemptReads).toBe(4);
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(api.attemptReads).toBe(5);
  });
});
