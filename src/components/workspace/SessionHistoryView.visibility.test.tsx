// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

const api = vi.hoisted(() => ({ attempts: vi.fn<() => Promise<any[]>>(), logs: vi.fn<() => Promise<any[]>>() }));
vi.mock("../../lib/ipc", async (original) => ({
  ...await original<object>(),
  listAttempts: () => api.attempts(),
  getChatLogs: () => api.logs(),
}));

import { SessionHistoryView } from "./SessionHistoryView";
import { useTaskTranscripts } from "../../stores/taskTranscripts";

const attempt = { id: "a", providerName: "CODEX_CLI", modelId: "test", role: "USER", turnCount: 1, status: "DONE", contextTokens: 0, createdAt: "2025-01-01T00:00:00Z" };
const log = { id: 1, messageId: "m", mainSessionId: "s", attemptId: "a", seq: 0, turnIndex: 0, role: "ASSISTANT", content: "stored transcript", contentTruncated: false, metaJson: "", createdAt: "2025-01-01T00:00:00Z" };
const deferred = <T,>() => { let resolve!: (value: T) => void; const promise = new Promise<T>((yes) => { resolve = yes; }); return { promise, resolve }; };

describe("SessionHistoryView polling visibility", () => {
  let host: HTMLDivElement;
  let root: Root;
  let visibility: DocumentVisibilityState;
  beforeEach(() => {
    (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers();
    visibility = "visible";
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
    api.attempts.mockReset().mockResolvedValue([attempt]);
    api.logs.mockReset().mockResolvedValue([log]);
    useTaskTranscripts.setState({ entries: {} });
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: class { observe() {} unobserve() {} disconnect() {} } });
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });
  const render = async () => act(async () => { root.render(createElement(SessionHistoryView, { sessionId: "s" })); await Promise.resolve(); await Promise.resolve(); });
  const transition = async (state: DocumentVisibilityState) => act(async () => {
    visibility = state;
    document.dispatchEvent(new Event("visibilitychange"));
    await Promise.resolve(); await Promise.resolve();
  });

  it("pauses both SQLite readers while hidden, preserves selection and live text, and resumes immediately", async () => {
    await render();
    expect(api.attempts).toHaveBeenCalledTimes(1);
    expect(api.logs).toHaveBeenCalledTimes(1);
    const positionTarget = host.querySelector("[data-message-role]")?.parentElement?.parentElement as HTMLElement;
    positionTarget.scrollTop = 47;
    await transition("hidden");
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(api.attempts).toHaveBeenCalledTimes(1);
    expect(api.logs).toHaveBeenCalledTimes(1);
    await act(async () => useTaskTranscripts.setState({ entries: { a: { request: { sessionId: "s", turnId: "a" } as never, messages: [{ id: "live", role: "assistant", text: "live while hidden" }] as never } } }));
    expect(host.textContent).toContain("live while hidden");
    expect(host.querySelector('[data-attempt-id="a"]')?.className).toContain("bg-active");
    expect(positionTarget.scrollTop).toBe(47);
    await transition("visible");
    expect(api.attempts).toHaveBeenCalledTimes(2);
    expect(api.logs).toHaveBeenCalledTimes(2);
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(host.textContent).toContain("live while hidden");
    expect(positionTarget.scrollTop).toBe(47);
  });

  it("cannot restart IPC after unmount", async () => {
    await render();
    await act(async () => root.unmount());
    const attempts = api.attempts.mock.calls.length;
    const logs = api.logs.mock.calls.length;
    visibility = "hidden"; document.dispatchEvent(new Event("visibilitychange"));
    visibility = "visible"; document.dispatchEvent(new Event("visibilitychange"));
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(api.attempts).toHaveBeenCalledTimes(attempts);
    expect(api.logs).toHaveBeenCalledTimes(logs);
  });

  it("allows an explicit refresh while hidden", async () => {
    await render();
    await transition("hidden");
    const next = deferred<any[]>();
    api.attempts.mockReturnValueOnce(next.promise);
    const button = host.querySelector('button[title="Refresh session history"]') ?? host.querySelector("button[title]");
    expect(button).toBeTruthy();
    await act(async () => { (button as HTMLButtonElement).click(); await Promise.resolve(); });
    expect(api.attempts).toHaveBeenCalledTimes(2);
    next.resolve([attempt]);
    await act(async () => { await next.promise; await Promise.resolve(); });
  });
});
