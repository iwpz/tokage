// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

const api = vi.hoisted(() => ({ reader: vi.fn<(session?: string, project?: string) => Promise<any[]>>() }));
vi.mock("../../lib/ipc", async (original) => ({
  ...await original<object>(),
  listAttempts: (session?: string, project?: string) => api.reader(session, project),
  getChatLogs: vi.fn(async () => []),
}));

import { SessionHistoryView } from "./SessionHistoryView";

const attempt = (id: string) => ({ id, providerName: "CODEX_CLI", modelId: "test", role: "USER", turnCount: 1, status: "DONE", contextTokens: 0, createdAt: "2025-01-01T00:00:00Z" });
const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

describe("SessionHistoryView list states", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
    api.reader.mockReset();
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); });
  const render = async (sessionId = "s") => act(async () => { root.render(createElement(SessionHistoryView, { sessionId })); await Promise.resolve(); });
  const settle = async () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });

  it("shows localized loading before successful empty data", async () => {
    const read = deferred<any[]>(); api.reader.mockReturnValueOnce(read.promise);
    await render();
    expect(host.querySelector('[role="status"]')?.textContent).toContain("Loading");
    expect(host.textContent).not.toContain("No underlying model interactions yet");
    await act(async () => read.resolve([])); await settle();
    expect(host.textContent).toContain("No underlying model interactions yet");
    expect(host.querySelector('[role="status"]')).toBeNull();
  });

  it("announces an initial failure without a selection and retries successfully", async () => {
    const first = deferred<any[]>(); const retry = deferred<any[]>();
    api.reader.mockReturnValueOnce(first.promise).mockReturnValueOnce(retry.promise);
    await render(); await act(async () => first.reject(new Error("offline"))); await settle();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("Could not load the session list");
    const button = [...host.querySelectorAll("button")].find((node) => node.textContent === "Retry") as HTMLButtonElement;
    expect(button).toBeTruthy();
    await act(async () => button.click());
    expect(host.querySelector('[role="status"]')?.textContent).toContain("Loading");
    expect(host.textContent).not.toContain("No underlying model interactions yet");
    await act(async () => retry.resolve([attempt("recovered")])); await settle();
    expect(host.querySelector('[data-attempt-id="recovered"]')).toBeTruthy();
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });

  it("retains attempts and selection through a refresh failure and ignores a stopped old scope", async () => {
    const initial = deferred<any[]>(); const failure = deferred<any[]>(); const oldPending = deferred<any[]>(); const nextScope = deferred<any[]>();
    api.reader.mockReturnValueOnce(initial.promise).mockReturnValueOnce(failure.promise).mockReturnValueOnce(oldPending.promise).mockReturnValueOnce(nextScope.promise);
    await render(); await act(async () => initial.resolve([attempt("kept")])); await settle();
    const selected = host.querySelector('[data-attempt-id="kept"]') as HTMLButtonElement;
    expect(selected.className).toContain("bg-active");
    const refresh = host.querySelector('button[title="Refresh session list"]') as HTMLButtonElement;
    await act(async () => refresh.click());
    await act(async () => failure.reject(new Error("offline"))); await settle();
    expect(host.querySelector('[data-attempt-id="kept"]')).toBeTruthy();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("Could not load the session list");
    const refreshAgain = host.querySelector('button[title="Refresh session list"]') as HTMLButtonElement;
    await act(async () => refreshAgain.click());
    expect(host.querySelector('[role="status"]')?.textContent).toContain("Loading");
    await render("new-session");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    await act(async () => nextScope.resolve([])); await settle();
    await act(async () => oldPending.reject(new Error("late old error"))); await settle();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.textContent).toContain("No underlying model interactions yet");
  });
});
