import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("react-i18next", async (importOriginal) => ({
  ...await importOriginal<typeof import("react-i18next")>(),
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("../../lib/ipc", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/ipc")>(),
  getChatLogs: vi.fn(async () => []),
  listAttempts: vi.fn(async () => ["attempt-a", "attempt-b"].map((id) => ({ id, providerName: "test", modelId: "model", role: "user", turnCount: 1, contextTokens: 1 }))),
}));

import { SessionHistoryView } from "./SessionHistoryView";
import { useTaskTranscripts } from "../../stores/taskTranscripts";

describe("SessionHistoryView scroll observer lifecycle", () => {
  let browser: Window;
  let host: HTMLDivElement;
  let root: Root;
  let constructions: number;
  let disconnections: number;
  let callbacks: ResizeObserverCallback[];
  let observed: Element[];

  beforeEach(() => {
    browser = new Window();
    Object.assign(globalThis, {
      window: browser,
      document: browser.document,
      HTMLElement: browser.HTMLElement,
      Element: browser.Element,
      MutationObserver: browser.MutationObserver,
    });
    Object.defineProperty(globalThis, "navigator", { value: browser.navigator, configurable: true });
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    constructions = 0;
    disconnections = 0;
    callbacks = [];
    observed = [];
    class TestResizeObserver {
      callback: ResizeObserverCallback;
      constructor(callback: ResizeObserverCallback) { constructions++; this.callback = callback; callbacks.push(callback); }
      observe(target: Element) { observed.push(target); }
      unobserve() {}
      disconnect() { disconnections++; }
    }
    Object.assign(globalThis, { ResizeObserver: TestResizeObserver });
    host = browser.document.createElement("div") as unknown as HTMLDivElement;
    browser.document.body.append(host as never);
    root = createRoot(host);
    useTaskTranscripts.setState({ entries: {} });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    browser.close();
  });

  async function mount() {
    await act(async () => {
      root.render(createElement(SessionHistoryView, { sessionId: "session" }));
      await Promise.resolve();
      await Promise.resolve();
    });
  }

  it("keeps one subscription through 100 transcript renders and observes added content", async () => {
    await mount();
    expect(constructions).toBe(1);
    const input = host.querySelector("input");
    for (let i = 0; i < 100; i++) {
      await act(async () => {
        useTaskTranscripts.setState((state) => ({ entries: { ...state.entries, "attempt-a": {
          request: { sessionId: "session", turnId: "attempt-a" } as never,
          messages: [{ id: "m", role: "assistant", text: `chunk ${i}` }] as never,
        } } }));
      });
      if (input) {
        await act(async () => {
          const setter = Object.getOwnPropertyDescriptor(browser.HTMLInputElement.prototype, "value")?.set;
          setter?.call(input, String(i));
          input.dispatchEvent(new browser.Event("input", { bubbles: true }) as unknown as Event);
        });
      }
    }
    expect(constructions).toBe(1);
    expect(observed.some((element) => element.closest(".space-y-4") !== null)).toBe(true);
    expect(disconnections).toBe(0);
  });

  it("disconnects on selection change and unmount, ignoring a queued stale callback", async () => {
    await mount();
    const stale = callbacks[0];
    const scroll = host.querySelector("[class*='overflow-y-auto']") as HTMLElement;
    const otherAttempt = host.querySelector("[data-attempt-id='attempt-b']") as HTMLButtonElement | null;
    await act(async () => {
      otherAttempt?.click();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(disconnections).toBe(1);
    expect(constructions).toBeGreaterThanOrEqual(2);
    if (scroll) {
      Object.defineProperties(scroll, { scrollHeight: { value: 500, configurable: true }, clientHeight: { value: 100, configurable: true }, scrollTop: { value: 0, writable: true, configurable: true } });
      stale([], {} as ResizeObserver);
      expect(scroll.scrollTop).toBe(0);
    }
    await act(async () => root.unmount());
    expect(disconnections).toBe(constructions);
  });
});
