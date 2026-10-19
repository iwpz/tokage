import { afterEach, describe, expect, it, vi } from "vitest";
import { pollHistory } from "./historyPoller";
afterEach(() => vi.useRealTimers());
describe("session history reader", () => {
  it("ignores an old attempt response after selection changes", async () => {
    let finish!: (value: string[]) => void;
    const accept = vi.fn();
    const old = pollHistory(() => new Promise<string[]>((r) => { finish = r; }), accept, vi.fn());
    old.stop();
    finish(["old attempt"]); await Promise.resolve();
    expect(accept).not.toHaveBeenCalled();
  });
  it("refreshes running messages, serializes reads, and keeps retrying after errors", async () => {
    vi.useFakeTimers();
    const read = vi.fn().mockRejectedValueOnce(new Error("DB busy")).mockResolvedValueOnce(["new chunk"]);
    const accept = vi.fn(), reject = vi.fn();
    const reader = pollHistory(read, accept, reject);
    await vi.advanceTimersByTimeAsync(1000);
    expect(reject).toHaveBeenCalledOnce(); expect(accept).toHaveBeenCalledWith(["new chunk"]);
    reader.stop(); await vi.advanceTimersByTimeAsync(5000);
    expect(read).toHaveBeenCalledTimes(2);
  });
  it("coalesces repeated refresh while a request is pending", async () => {
    vi.useFakeTimers();
    let finish!: (value: number) => void;
    const read = vi.fn().mockImplementationOnce(() => new Promise<number>((r) => { finish = r; })).mockResolvedValue(2);
    const reader = pollHistory(read, vi.fn(), vi.fn());
    void reader.refresh(); void reader.refresh(); expect(read).toHaveBeenCalledOnce();
    finish(1); await vi.advanceTimersByTimeAsync(0); expect(read).toHaveBeenCalledTimes(2);
    reader.stop();
  });
});
