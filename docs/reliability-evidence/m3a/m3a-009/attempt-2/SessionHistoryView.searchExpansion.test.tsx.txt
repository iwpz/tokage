// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

const api = vi.hoisted(() => ({ attempts: [] as any[], attemptReads: 0, logReads: 0 }));
vi.mock("../../lib/ipc", async (original) => ({
  ...await original<object>(),
  listAttempts: vi.fn(async () => { api.attemptReads++; return api.attempts; }),
  getChatLogs: vi.fn(async (attemptId: string) => { api.logReads++; return [transcript(attemptId)]; }),
}));

import { SessionHistoryView } from "./SessionHistoryView";
import { useTaskTranscripts } from "../../stores/taskTranscripts";

const attempt = (id: string, fields: Record<string, unknown>) => ({ id, providerName: "CLAUDE_CODE", modelId: "claude-haiku-5-5", role: "EXECUTOR", turnCount: 1, status: "DONE", contextTokens: 0, createdAt: "2025-01-01T00:00:00Z", taskId: null, ...fields });
const transcript = (attemptId: string) => ({ id: 1, messageId: `m-${attemptId}`, mainSessionId: "s", attemptId, seq: 0, turnIndex: 0, role: "ASSISTANT", content: `transcript ${attemptId}`, contentTruncated: false, metaJson: "", createdAt: "2025-01-01T00:00:00Z" });
const dataset = () => [
  attempt("att-a", { modelId: "claude-test", role: "EXECUTOR", status: "DONE", taskId: 7 }),
  attempt("att-b", { modelId: "claude-haiku-5-5", role: "PLANNER", status: "RUNNING", taskId: 8 }),
  attempt("att-c", { providerName: "CODEX_CLI", modelId: "gpt-test", role: "REVIEWER", status: "FAILED", taskId: 42 }),
  attempt("att-d", { providerName: "CODEX_CLI", modelId: "gpt-mini", role: "TESTER", status: "DONE", taskId: 43 }),
];

describe("SessionHistoryView search expansion", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    vi.useFakeTimers();
    (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
    api.attempts = dataset(); api.attemptReads = 0; api.logReads = 0;
    useTaskTranscripts.setState({ entries: {} });
    Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: class { observe() {} unobserve() {} disconnect() {} } });
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

  const flush = (ms = 0) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
  const render = () => act(async () => { root.render(createElement(SessionHistoryView, { sessionId: "s" })); await vi.advanceTimersByTimeAsync(0); await vi.advanceTimersByTimeAsync(0); });
  const input = () => host.querySelector<HTMLInputElement>("input[aria-label]")!;
  const attemptNode = (id: string) => host.querySelector<HTMLButtonElement>(`[data-attempt-id="${id}"]`)!;
  const attemptNodes = () => [...host.querySelectorAll<HTMLElement>("[data-attempt-id]")];
  const attemptIds = () => attemptNodes().map((node) => node.dataset.attemptId);
  const visibleIds = () => attemptNodes().filter((node) => !node.closest("[hidden]")).map((node) => node.dataset.attemptId);
  const selectedIds = () => attemptNodes().filter((node) => node.getAttribute("aria-current") === "true").map((node) => node.dataset.attemptId);
  const toggle = (label: string) => [...host.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")].find((node) => node.textContent?.includes(label))!;
  const expanded = (label: string) => toggle(label).getAttribute("aria-expanded");
  const click = (node: HTMLElement) => act(async () => node.click());
  const type = (value: string) => act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input(), value);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
  const clearWithButton = () => act(async () => (host.querySelector('[aria-label="Clear search"]') as HTMLButtonElement).click());
  const pressEscape = () => act(async () => input().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  const expectSearchExpanded = () => {
    const toggles = [...host.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")];
    expect(toggles.length).toBeGreaterThan(0);
    for (const node of toggles) { expect(node.getAttribute("aria-expanded")).toBe("true"); expect(node.disabled).toBe(true); }
  };
  const expectStableControls = () => {
    const ids = [...host.querySelectorAll<HTMLElement>("[id]")].map((node) => node.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const node of host.querySelectorAll<HTMLElement>("button[aria-controls]")) expect(document.getElementById(node.getAttribute("aria-controls")!)).not.toBeNull();
  };
  // Collapses claude-test, the Codex CLI group, and gpt-mini, leaving mixed saved preferences.
  const collapseMixed = async () => {
    await click(toggle("claude-test"));
    await click(toggle("Codex CLI"));
    await click(toggle("gpt-mini"));
  };
  const expectMixedRestored = () => {
    expect(expanded("Claude Code CLI")).toBe("true");
    expect(expanded("claude-test")).toBe("false");
    expect(expanded("claude-haiku-5-5")).toBe("true");
    expect(expanded("Codex CLI")).toBe("false");
    expect(expanded("gpt-test")).toBe("true");
    expect(expanded("gpt-mini")).toBe("false");
    expect(visibleIds()).toEqual(["att-b"]);
    expectStableControls();
  };

  it("exposes matches through non-hidden ancestors when both groups were manually collapsed", async () => {
    await render();
    for (const label of ["claude-test", "claude-haiku-5-5", "gpt-test", "gpt-mini"]) await click(toggle(label));
    for (const label of ["Claude Code CLI", "Codex CLI"]) await click(toggle(label));
    expect(visibleIds()).toEqual([]);
    expect(expanded("Codex CLI")).toBe("false");

    const cases = [
      ["42", ["att-c"]],
      ["gpt-test", ["att-c"]],
      ["reviewer", ["att-c"]],
      ["failed", ["att-c"]],
      ["claude-haiku", ["att-b"]],
      ["planner", ["att-b"]],
      ["running", ["att-b"]],
    ] as const;
    for (const [query, ids] of cases) {
      await type(query);
      expect(visibleIds()).toEqual(ids);
      expectSearchExpanded();
      expectStableControls();
    }
    await clearWithButton();
    expect(visibleIds()).toEqual([]);
    expect(expanded("Codex CLI")).toBe("false");
  });

  it("keeps results and saved preferences unchanged when toggles are clicked during search", async () => {
    await render();
    await collapseMixed();
    await type("test");
    expect(visibleIds()).toEqual(["att-a", "att-c", "att-d"]);
    expectSearchExpanded();
    for (const label of ["Claude Code CLI", "claude-test", "Codex CLI", "gpt-mini"]) await click(toggle(label));
    expect(visibleIds()).toEqual(["att-a", "att-c", "att-d"]);
    expectSearchExpanded();
    await clearWithButton();
    expectMixedRestored();
  });

  it("treats a whitespace-only query as normal mode without forcing collapsed groups open", async () => {
    await render();
    await collapseMixed();
    await type("   ");
    expect(attemptIds()).toEqual(["att-a", "att-b", "att-c", "att-d"]);
    expect(visibleIds()).toEqual(["att-b"]);
    expect(expanded("claude-test")).toBe("false");
    expect(expanded("Codex CLI")).toBe("false");
    expect(toggle("Codex CLI").disabled).toBe(false);
    expectStableControls();
  });

  it("restores exact mixed preferences after the clear button and keeps focus in the search input", async () => {
    await render();
    await collapseMixed();
    await type("test");
    expect(visibleIds()).toEqual(["att-a", "att-c", "att-d"]);
    expectSearchExpanded();
    await clearWithButton();
    expect(input().value).toBe("");
    expect(document.activeElement).toBe(input());
    expectMixedRestored();
  });

  it("restores exact mixed preferences after Escape and keeps focus in the search input", async () => {
    await render();
    await collapseMixed();
    await type("test");
    expect(visibleIds()).toEqual(["att-a", "att-c", "att-d"]);
    expectSearchExpanded();
    await pressEscape();
    expect(input().value).toBe("");
    expect(document.activeElement).toBe(input());
    expectMixedRestored();
  });

  it("keeps saved preferences through a zero-match search and a later clear", async () => {
    await render();
    await collapseMixed();
    await type("no-such-attempt");
    expect(attemptIds()).toEqual([]);
    expect(host.textContent).toContain("No matching sessions");
    await clearWithButton();
    expectMixedRestored();
  });

  it("keeps selection, transcript, and IPC reads independent of search results", async () => {
    await render();
    await click(attemptNode("att-c"));
    expect(selectedIds()).toEqual(["att-c"]);
    expect(host.textContent).toContain("transcript att-c");
    const reads = { attempts: api.attemptReads, logs: api.logReads };
    await type("gpt");
    expect(visibleIds()).toEqual(["att-c", "att-d"]);
    expect(selectedIds()).toEqual(["att-c"]);
    await type("no-such-attempt");
    expect(host.textContent).toContain("transcript att-c");
    await pressEscape();
    expect(attemptIds()).toEqual(["att-a", "att-b", "att-c", "att-d"]);
    expect(selectedIds()).toEqual(["att-c"]);
    expect(host.textContent).toContain("transcript att-c");
    expect({ attempts: api.attemptReads, logs: api.logReads }).toEqual(reads);
  });

  it("keeps current matches and saved preferences through polling while searching", async () => {
    await render();
    await collapseMixed();
    await type("test");
    api.attempts = [...dataset(), attempt("att-e", { providerName: "CURSOR_CLI", modelId: "cursor-test", role: "EXECUTOR", status: "DONE", taskId: 9 })];
    await flush(1000);
    expect(visibleIds()).toEqual(["att-a", "att-c", "att-d", "att-e"]);
    expectSearchExpanded();
    await clearWithButton();
    expect(expanded("claude-test")).toBe("false");
    expect(expanded("Codex CLI")).toBe("false");
    expect(expanded("gpt-mini")).toBe("false");
    expect(expanded("Cursor CLI")).toBe("true");
    expect(expanded("cursor-test")).toBe("true");
    expect(visibleIds()).toEqual(["att-b", "att-e"]);
    expectStableControls();
  });
});
