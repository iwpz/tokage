// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

const api = vi.hoisted(() => ({ attempts: [] as any[], logs: [] as any[], failLogs: false }));
const resizeObservers: Array<{ fire: () => void }> = [];
const observerCounts = { constructed: 0, disconnected: 0, observed: [] as Element[], unobserved: [] as Element[] };
const logUpdates: Array<(rows: any[]) => void> = [];
vi.mock("../../lib/ipc", async (original) => ({
  ...await original<object>(),
  listAttempts: vi.fn(async () => api.attempts),
  getChatLogs: vi.fn(async () => { if (api.failLogs) throw new Error("read failed"); return api.logs; }),
}));
vi.mock("../../lib/historyPoller", () => ({
  pollHistory: (read: () => Promise<any>, onData: (data: any) => void, onError: () => void) => {
    let stopped = false;
    void read().then((data) => { if (!stopped) onData(data); }, () => { if (!stopped) onError(); });
    if (read.toString().includes("getChatLogs")) logUpdates.push(onData);
    return { stop: () => { stopped = true; }, refresh: async () => {} };
  },
}));

import { SessionHistoryView } from "./SessionHistoryView";
import { useTaskTranscripts } from "../../stores/taskTranscripts";

const attempt = (id: string) => ({ id, providerName: "CLAUDE_CODE", modelId: "test", role: "USER", turnCount: 1, status: "DONE", contextTokens: 0, createdAt: "2025-01-01T00:00:00Z" });
const log = (id: string, content: string, metaJson = "") => ({ id: 1, messageId: id, mainSessionId: "s", attemptId: "a", seq: 0, turnIndex: 0, role: "ASSISTANT", content, contentTruncated: false, metaJson, createdAt: "2025-01-01T00:00:00Z" });

describe("SessionHistoryView scroll intent", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
    api.attempts = [attempt("a")]; api.logs = [log("first", "first output")]; api.failLogs = false;
    useTaskTranscripts.setState({ entries: {} });
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    resizeObservers.length = 0; logUpdates.length = 0;
    observerCounts.constructed = 0; observerCounts.disconnected = 0;
    observerCounts.observed = []; observerCounts.unobserved = [];
    Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: class {
      private callback: ResizeObserverCallback;
      constructor(callback: ResizeObserverCallback) { this.callback = callback; observerCounts.constructed++; resizeObservers.push({ fire: () => this.callback([], this as unknown as ResizeObserver) }); }
      observe(target: Element) { observerCounts.observed.push(target); }
      unobserve(target: Element) { observerCounts.unobserved.push(target); }
      disconnect() { observerCounts.disconnected++; }
    } });
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); });
  const render = async () => { await act(async () => { root.render(createElement(SessionHistoryView, { sessionId: "s" })); await new Promise((resolve) => setTimeout(resolve, 0)); }); };
  const transcript = () => host.querySelector("[data-message-role]")?.parentElement?.parentElement as HTMLElement;
  const dimensions = (node: HTMLElement, height: number, client = 100) => {
    Object.defineProperties(node, { scrollHeight: { configurable: true, value: height }, clientHeight: { configurable: true, value: client } });
  };

  it("renders the selected attempt and initially follows its latest output", async () => {
    await render();
    const container = transcript(); dimensions(container, 600);
    act(() => resizeObservers.at(-1)?.fire());
    expect(host.textContent).toContain("first output");
    expect(container.scrollTop).toBe(600);
  });

  it("shows an accessible return button after the reader scrolls up", async () => {
    await render();
    const container = transcript(); dimensions(container, 600);
    container.scrollTop = 80;
    await act(async () => container.dispatchEvent(new Event("scroll")));
    const button = [...host.querySelectorAll("button")].find((node) => node.textContent?.includes("Return to latest"));
    expect(button).toBeTruthy(); expect(button?.getAttribute("aria-label")).toBeTruthy();
    dimensions(container, 700);
    act(() => resizeObservers.at(-1)?.fire());
    await act(async () => button?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(container.scrollTop).toBe(700);
  });

  it("keeps the reader's position when refreshed output arrives", async () => {
    await render();
    const container = transcript(); dimensions(container, 600);
    container.scrollTop = 80;
    await act(async () => container.dispatchEvent(new Event("scroll")));
    await act(async () => logUpdates.at(-1)?.([log("second", "new streaming output")]));
    dimensions(container, 720);
    act(() => resizeObservers.at(-1)?.fire());
    expect(container.scrollTop).toBe(80);
    expect(host.textContent).toContain("new streaming output");
  });

  it("renders empty and failed reads without retaining a stale return control", async () => {
    api.logs = []; await render();
    expect(host.textContent).toContain("No message text retained");
    expect(host.querySelector('[aria-label="Return to latest"]')).toBeNull();
  });

  it("renders a read error without exposing the previous attempt's scroll control", async () => {
    api.failLogs = true;
    await render();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("Could not refresh session history");
    expect(host.querySelector('[aria-label="Return to latest"]')).toBeNull();
  });

  it("resets scroll intent when selecting a different attempt", async () => {
    api.attempts = [attempt("a"), { ...attempt("b"), createdAt: "2025-01-02T00:00:00Z" }];
    await render();
    const container = transcript(); dimensions(container, 600);
    container.scrollTop = 20;
    await act(async () => container.dispatchEvent(new Event("scroll")));
    const selectB = host.querySelector('[data-attempt-id="b"]') as HTMLButtonElement;
    await act(async () => selectB.click());
    await act(async () => logUpdates.at(-1)?.([log("latest-b", "attempt B latest")]));
    dimensions(transcript(), 800);
    act(() => resizeObservers.at(-1)?.fire());
    expect(transcript().scrollTop).toBe(800);
    expect(host.textContent).toContain("attempt B latest");
  });

  it("keeps one observer through 100 rendered transcript updates and follows real content growth", async () => {
    await render();
    const container = transcript(); dimensions(container, 600);
    act(() => resizeObservers.at(-1)?.fire());
    expect(observerCounts.constructed).toBe(1);
    for (let index = 0; index < 100; index++) {
      await act(async () => useTaskTranscripts.setState({ entries: { a: {
        request: { sessionId: "s", turnId: "a" } as never,
        messages: [{ id: "stream", role: "assistant", text: `rendered stream chunk ${index}` }] as never,
      } } }));
      expect(host.textContent).toContain(`rendered stream chunk ${index}`);
    }
    expect(observerCounts.constructed).toBe(1);
    dimensions(container, 840);
    act(() => resizeObservers.at(-1)?.fire());
    expect(container.scrollTop).toBe(840);
  });

  it("unobserves removed transcript targets and ignores queued callbacks after selection change and unmount", async () => {
    api.attempts = [attempt("a"), attempt("b")];
    await render();
    const oldContainer = transcript(); dimensions(oldContainer, 600);
    const stale = resizeObservers.at(-1)?.fire;
    const removedTarget = host.querySelector("[data-message-role]") as Element;
    await act(async () => logUpdates.at(-1)?.([log("replacement", "replacement transcript") ]));
    await act(async () => { await Promise.resolve(); });
    expect(host.textContent).toContain("replacement transcript");
    expect(observerCounts.unobserved).toContain(removedTarget);

    const selectB = host.querySelector('[data-attempt-id="b"]') as HTMLButtonElement;
    await act(async () => selectB.click());
    const newContainer = transcript(); dimensions(newContainer, 700);
    newContainer.scrollTop = 25;
    const textBefore = host.textContent;
    await act(async () => stale?.());
    expect(newContainer.scrollTop).toBe(25);
    expect(host.textContent).toBe(textBefore);
    expect(observerCounts.disconnected).toBeGreaterThanOrEqual(1);
    await act(async () => root.unmount());
    const disconnectedAtUnmount = observerCounts.disconnected;
    await act(async () => resizeObservers.at(-1)?.fire());
    expect(newContainer.scrollTop).toBe(25);
    expect(observerCounts.disconnected).toBe(disconnectedAtUnmount);
    expect(observerCounts.disconnected).toBe(observerCounts.constructed);
  });
});
