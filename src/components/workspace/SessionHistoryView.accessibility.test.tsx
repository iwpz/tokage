// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";

const api = vi.hoisted(() => ({
  attempts: [] as any[], logs: [] as any[], scopes: [] as any[],
}));
vi.mock("../../lib/ipc", async (original) => ({
  ...await original<object>(),
  listAttempts: vi.fn(async (sessionId?: string, projectId?: string) => {
    api.scopes.push([sessionId, projectId]); return api.attempts;
  }),
  getChatLogs: vi.fn(async (attemptId: string) => api.logs.map((row) => ({ ...row, attemptId, content: `${row.content} ${attemptId}` }))),
}));

import { SessionHistoryView } from "./SessionHistoryView";

const attempt = (id: string) => ({ id, providerName: "CODEX_CLI", modelId: "gpt-test", role: "USER", turnCount: 1, status: "DONE", contextTokens: 0, createdAt: "2025-01-01T00:00:00Z" });
const log = (content: string) => ({ id: 1, messageId: "m", mainSessionId: "s", seq: 0, turnIndex: 0, role: "ASSISTANT", content, contentTruncated: false, metaJson: "{}", createdAt: "2025-01-01T00:00:00Z" });

describe("SessionHistoryView accessibility state", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    vi.useFakeTimers();
    (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
    api.attempts = [attempt("a"), attempt("b")]; api.logs = [log("transcript a")]; api.scopes = [];
    Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: class { observe() {} unobserve() {} disconnect() {} } });
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });
  const render = async (children: ReactNode = createElement(SessionHistoryView, { sessionId: "s", projectId: "p" })) => {
    await act(async () => { root.render(children); await Promise.resolve(); await Promise.resolve(); });
  };
  const button = (name: string) => [...host.querySelectorAll("button")].find((item) => item.textContent?.trim() === name)!;

  it("exposes scope, group, selection, polling, and transcript states with stable unique panels", async () => {
    await render();
    expect(host.querySelectorAll('button[aria-pressed="true"]')).toHaveLength(1);
    expect(button("Current session").getAttribute("aria-pressed")).toBe("true");
    expect(button("All in this project").getAttribute("aria-pressed")).toBe("false");
    const groupButtons = [...host.querySelectorAll<HTMLButtonElement>('button[aria-expanded]')];
    const provider = groupButtons[0];
    const providerId = provider.getAttribute("aria-controls")!;
    expect(provider.getAttribute("aria-expanded")).toBe("true");
    expect(host.querySelector(`#${CSS.escape(providerId)}`)).toBeTruthy();
    const model = groupButtons[1];
    const modelId = model.getAttribute("aria-controls")!;
    expect(model.getAttribute("aria-expanded")).toBe("true");
    expect(host.querySelector(`#${CSS.escape(modelId)}`)).toBeTruthy();
    expect(host.querySelectorAll('[aria-current="true"]')).toHaveLength(1);
    expect(host.textContent).toContain("transcript a a");
    expect(api.scopes).toContainEqual(["s", undefined]);

    await act(async () => (host.querySelector('[data-attempt-id="b"]') as HTMLButtonElement).click());
    expect(host.querySelector('[data-attempt-id="b"]')?.getAttribute("aria-current")).toBe("true");
    expect(host.querySelector('[data-attempt-id="a"]')?.hasAttribute("aria-current")).toBe(false);
    expect(host.textContent).toContain("transcript a b");

    provider.focus();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(document.activeElement).toBe(provider);
    expect(provider.getAttribute("aria-controls")).toBe(providerId);
    expect(host.querySelectorAll('[aria-current="true"]')).toHaveLength(1);

    await act(async () => provider.click());
    expect(provider.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector(`#${CSS.escape(providerId)}`)?.hasAttribute("hidden")).toBe(true);
    expect(host.querySelector(`#${CSS.escape(providerId)} button`)).toBeTruthy();
    await act(async () => provider.click());
    expect(provider.getAttribute("aria-expanded")).toBe("true");
    expect(provider.getAttribute("aria-controls")).toBe(providerId);
    await act(async () => model.click());
    expect(model.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector(`#${CSS.escape(modelId)}`)?.hasAttribute("hidden")).toBe(true);
    await act(async () => model.click());
    expect(model.getAttribute("aria-expanded")).toBe("true");
    expect(model.getAttribute("aria-controls")).toBe(modelId);
    expect(host.querySelectorAll("[id]").length).toBe(new Set([...host.querySelectorAll<HTMLElement>("[id]")].map((node) => node.id)).size);
  });

  it("switches scope using native buttons and keeps independent IDs across mounted views", async () => {
    await render(createElement("div", null,
      createElement(SessionHistoryView, { sessionId: "s", projectId: "p" }),
      createElement(SessionHistoryView, { sessionId: "s2", projectId: "p" }),
    ));
    const allProviderIds = [...host.querySelectorAll<HTMLButtonElement>('button[aria-controls]')].map((node) => node.getAttribute("aria-controls"));
    expect(new Set(allProviderIds).size).toBe(allProviderIds.length);
    const projectButtons = [...host.querySelectorAll("button")].filter((node) => node.textContent?.trim() === "All in this project");
    await act(async () => projectButtons[0].click());
    expect(projectButtons[0].getAttribute("aria-pressed")).toBe("true");
    expect(projectButtons[1].getAttribute("aria-pressed")).toBe("false");
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });
    expect(api.scopes).toContainEqual([undefined, "p"]);
    expect(host.querySelectorAll('[role="tree"], [role="treeitem"]')).toHaveLength(0);
  });
});
