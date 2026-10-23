// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

const api = vi.hoisted(() => ({ attempts: [] as any[], logs: [] as any[], failLogs: false, attemptReads: 0, logReads: 0 }));
const resizeObservers: Array<{ fire: () => void }> = [];
const observerCounts = { constructed: 0, disconnected: 0, observed: [] as Element[], unobserved: [] as Element[] };
const logUpdates: Array<(rows: any[]) => void> = [];
const searchResizeCallbacks: Array<ResizeObserverCallback> = [];
vi.mock("../../lib/ipc", async (original) => ({
  ...await original<object>(),
  listAttempts: vi.fn(async () => { api.attemptReads++; return api.attempts; }),
  getChatLogs: vi.fn(async () => { api.logReads++; if (api.failLogs) throw new Error("read failed"); return api.logs; }),
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
import i18n from "../../i18n";

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

  it("labels internal notices without a default model or executor", async () => {
    api.attempts = [{ ...attempt("s|TOKAGE_INTERNAL|"), providerName: "TOKAGE_INTERNAL", modelId: "", role: "EXECUTOR" }];
    api.logs = [{ ...log("notice", "Pipeline starting"), role: "SYSTEM" }];
    await render();
    expect(host.textContent).toContain("Tokage notices");
    expect(host.textContent).toContain("No recorded model");
    expect(host.textContent).toContain("Pipeline starting");
    expect(host.textContent).not.toContain("default");
    expect(host.textContent).not.toContain("Executor");
  });

  it("keeps legacy unattributed messages visible without disguising unknown task providers", async () => {
    api.attempts = [
      { ...attempt("s|UNKNOWN|"), providerName: "UNKNOWN", modelId: "", role: "EXECUTOR" },
      { ...attempt("real-task"), providerName: "UNKNOWN", modelId: "real-model", taskId: 1 },
    ];
    await render();
    expect(host.textContent).toContain("Unattributed messages (legacy)");
    expect(host.textContent).toContain("first output");
    expect(host.textContent).toContain("UNKNOWN");
    expect(host.textContent).toContain("Real Model");
    expect(host.textContent).not.toContain("default");
  });

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

describe("SessionHistoryView search", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
    api.attemptReads = 0; api.logReads = 0;
    api.attempts = [
      { ...attempt("raw-attempt-one"), providerName: "CODEX_CLI", modelId: "gpt-test", role: "PLANNER", status: "RUNNING", taskId: 42 },
      { ...attempt("raw-attempt-two"), providerName: "CLAUDE_CODE", modelId: "claude-test", role: "EXECUTOR", status: "DONE", taskId: null },
      { ...attempt("s|TOKAGE_INTERNAL|"), providerName: "TOKAGE_INTERNAL", modelId: "", role: "EXECUTOR", taskId: null },
    ];
    api.logs = [log("kept", "selected transcript")]; api.failLogs = false;
    useTaskTranscripts.setState({ entries: {} });
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    searchResizeCallbacks.length = 0;
    Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: class {
      constructor(callback: ResizeObserverCallback) { searchResizeCallbacks.push(callback); }
      observe() {} unobserve() {} disconnect() {}
    } });
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); });
  const render = async () => { await act(async () => { root.render(createElement(SessionHistoryView, { sessionId: "s" })); await new Promise((resolve) => setTimeout(resolve, 0)); }); };
  const attemptIds = () => [...host.querySelectorAll<HTMLElement>("[data-attempt-id]")].map((node) => node.dataset.attemptId);
  const search = () => host.querySelector<HTMLInputElement>('input[aria-label]')!;
  const enter = async (value: string) => act(async () => {
    const input = search();
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });

  it("filters rendered attempts by task, status, raw and localized labels, and safely clears", async () => {
    await render();
    expect(search().getAttribute("aria-label")).toBeTruthy();
    expect(attemptIds()).toEqual(["raw-attempt-one", "raw-attempt-two", "s|TOKAGE_INTERNAL|"]);
    expect(host.querySelector('[aria-label="Clear search"]')).toBeNull();
    await enter(" 42 "); expect(attemptIds()).toEqual(["raw-attempt-one"]);
    await enter("running"); expect(attemptIds()).toEqual(["raw-attempt-one"]);
    await enter("openai codex cli"); expect(attemptIds()).toEqual(["raw-attempt-one"]);
    await enter("CODEX_CLI"); expect(attemptIds()).toEqual(["raw-attempt-one"]);
    await enter("gpt-test"); expect(attemptIds()).toEqual(["raw-attempt-one"]);
    await enter("raw-attempt-one"); expect(attemptIds()).toEqual(["raw-attempt-one"]);
    await enter("planner"); expect(attemptIds()).toEqual(["raw-attempt-one"]);
    await act(async () => { await i18n.changeLanguage("zh"); });
    await enter("规划者"); expect(attemptIds()).toEqual(["raw-attempt-one"]);
    await enter("Tokage 内部通知"); expect(attemptIds()).toEqual(["s|TOKAGE_INTERNAL|"]);
    await act(async () => { await i18n.changeLanguage("en"); });
    await enter("tokage notices"); expect(attemptIds()).toEqual(["s|TOKAGE_INTERNAL|"]);
    await enter("missing"); expect(attemptIds()).toEqual([]);
    await enter("   "); expect(attemptIds()).toEqual(["raw-attempt-one", "raw-attempt-two", "s|TOKAGE_INTERNAL|"]);
    expect(api.attemptReads).toBe(1);
    expect(api.logReads).toBe(1);
    expect(host.querySelector('[aria-label="Clear search"]')).toBeTruthy();
    await act(async () => (host.querySelector('[aria-label="Clear search"]') as HTMLButtonElement).click());
    expect(search().value).toBe(""); expect(document.activeElement).toBe(search());
    expect(attemptIds()).toEqual(["raw-attempt-one", "raw-attempt-two", "s|TOKAGE_INTERNAL|"]);
    expect(api.attemptReads).toBe(1); expect(api.logReads).toBe(1);
  });

  it("Escape clears search without changing selection, transcript, or scroll intent", async () => {
    await render();
    const selected = host.querySelector('[data-attempt-id="raw-attempt-one"]') as HTMLButtonElement;
    await act(async () => selected.click());
    const transcript = host.querySelector('[data-message-role]')?.parentElement?.parentElement as HTMLElement;
    Object.defineProperties(transcript, { scrollHeight: { configurable: true, value: 600 }, clientHeight: { configurable: true, value: 100 } });
    transcript.scrollTop = 50;
    await act(async () => transcript.dispatchEvent(new Event("scroll")));
    const input = search();
    await enter("no-match");
    expect(attemptIds()).toEqual([]);
    expect(host.textContent).toContain("selected transcript");
    expect(transcript.scrollTop).toBe(50);
    await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(search().value).toBe(""); expect(document.activeElement).toBe(search());
    expect(attemptIds()).toEqual(["raw-attempt-one", "raw-attempt-two", "s|TOKAGE_INTERNAL|"]);
    expect(host.querySelector('[data-attempt-id="raw-attempt-one"]')?.className).toContain("bg-active");
    expect(transcript.scrollTop).toBe(50);
    expect(host.textContent).toContain("selected transcript");
    await act(async () => logUpdates.at(-1)?.([log("growth", "streamed growth after filtering")]));
    Object.defineProperty(transcript, "scrollHeight", { configurable: true, value: 720 });
    act(() => searchResizeCallbacks.at(-1)?.([], {} as ResizeObserver));
    expect(transcript.scrollTop).toBe(50);
    await enter("still-no-match");
    await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    act(() => searchResizeCallbacks.at(-1)?.([], {} as ResizeObserver));
    expect(transcript.scrollTop).toBe(50);
    expect(host.textContent).toContain("streamed growth after filtering");
    expect(api.attemptReads).toBe(1); expect(api.logReads).toBe(1);
  });
});
