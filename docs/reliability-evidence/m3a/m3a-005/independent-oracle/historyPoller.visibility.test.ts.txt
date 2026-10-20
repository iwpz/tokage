import { afterEach, describe, expect, it, vi } from "vitest";
import { pollHistory } from "./historyPoller";

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("history polling visibility", () => {
  it("runs the hidden initial read, pauses automatic reads for 60 seconds, and refreshes manually", async () => {
    vi.useFakeTimers();
    const listeners = new Set<EventListener>();
    let visibilityState: DocumentVisibilityState = "hidden";
    vi.stubGlobal("document", {
      get visibilityState() { return visibilityState; },
      addEventListener: (_type: string, listener: EventListener) => listeners.add(listener),
      removeEventListener: (_type: string, listener: EventListener) => listeners.delete(listener),
    });
    const read = vi.fn(async () => read.mock.calls.length);
    const poller = pollHistory(read, vi.fn(), vi.fn());
    await Promise.resolve();
    expect(read).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(read).toHaveBeenCalledTimes(1);
    await poller.refresh();
    expect(read).toHaveBeenCalledTimes(2);
    visibilityState = "visible";
    listeners.forEach((listener) => listener(new Event("visibilitychange")));
    await Promise.resolve();
    expect(read).toHaveBeenCalledTimes(3);
    poller.stop();
    expect(listeners.size).toBe(0);
  });

  it("coalesces refresh and visibility events during a pending read and cancels hidden follow-ups", async () => {
    vi.useFakeTimers();
    const listeners = new Set<EventListener>();
    let visibilityState: DocumentVisibilityState = "visible";
    vi.stubGlobal("document", {
      get visibilityState() { return visibilityState; },
      addEventListener: (_type: string, listener: EventListener) => listeners.add(listener),
      removeEventListener: (_type: string, listener: EventListener) => listeners.delete(listener),
    });
    let finish!: () => void;
    const read = vi.fn().mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; })).mockResolvedValue(undefined);
    const poller = pollHistory(read, vi.fn(), vi.fn());
    visibilityState = "hidden";
    listeners.forEach((listener) => listener(new Event("visibilitychange")));
    visibilityState = "visible";
    listeners.forEach((listener) => listener(new Event("visibilitychange")));
    visibilityState = "hidden";
    listeners.forEach((listener) => listener(new Event("visibilitychange")));
    finish();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(read).toHaveBeenCalledTimes(1);
    visibilityState = "visible";
    listeners.forEach((listener) => listener(new Event("visibilitychange")));
    await Promise.resolve();
    expect(read).toHaveBeenCalledTimes(2);
    poller.stop();
  });
});
