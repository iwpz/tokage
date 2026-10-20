import { afterEach, describe, expect, it, vi } from "vitest";
import { pollHistory } from "./historyPoller";

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("history polling backoff", () => {
  it("backs off from settled failures with exact boundaries, caps, and resets on success", async () => {
    vi.useFakeTimers();
    const read = vi.fn().mockRejectedValue(new Error("busy"));
    const poller = pollHistory(read, vi.fn(), vi.fn());
    await Promise.resolve();
    const delays = [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000];
    for (const [index, delay] of delays.entries()) {
      expect(read).toHaveBeenCalledTimes(index + 1);
      await vi.advanceTimersByTimeAsync(delay - 1);
      expect(read).toHaveBeenCalledTimes(index + 1);
      await vi.advanceTimersByTimeAsync(1);
      expect(read).toHaveBeenCalledTimes(index + 2);
    }
    poller.stop();
  });

  it("starts delay after settlement, immediately bypasses stale retry, and resets after success", async () => {
    vi.useFakeTimers();
    let finish!: (value: string) => void;
    const read = vi.fn()
      .mockRejectedValueOnce(new Error("first"))
      .mockImplementationOnce(() => new Promise<string>((resolve) => { finish = resolve; }))
      .mockResolvedValue("ok");
    const accept = vi.fn();
    const poller = pollHistory(read, accept, vi.fn());
    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(read).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(50_000);
    expect(read).toHaveBeenCalledTimes(2);
    finish("ok"); await Promise.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(accept).toHaveBeenCalledWith("ok");
    await vi.advanceTimersByTimeAsync(999);
    expect(read).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(read).toHaveBeenCalledTimes(3);
    poller.stop();
  });

  it("keeps base interval compatibility and failure state independent per reader", async () => {
    vi.useFakeTimers();
    const failing = vi.fn().mockRejectedValue(new Error("busy"));
    const healthy = vi.fn().mockResolvedValue("healthy");
    const a = pollHistory(failing, vi.fn(), vi.fn(), 200);
    const b = pollHistory(healthy, vi.fn(), vi.fn(), 40_000);
    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(200);
    expect(failing).toHaveBeenCalledTimes(2);
    expect(healthy).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(39_800);
    expect(healthy).toHaveBeenCalledTimes(2);
    a.stop(); b.stop();
  });
});
