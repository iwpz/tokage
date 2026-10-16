import { describe, expect, it, vi } from "vitest";
import { createWindowCloseHandler } from "./windowClose";

describe("window close persistence boundary", () => {
  it("prevents close synchronously, waits for writes, and coalesces repeated close requests", async () => {
    let finish!: () => void;
    const flush = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    const destroy = vi.fn(async () => {});
    const close = createWindowCloseHandler(flush, destroy);
    const event = { preventDefault: vi.fn() };
    const first = close(event);
    const second = close(event);
    expect(event.preventDefault).toHaveBeenCalledTimes(2);
    expect(flush).toHaveBeenCalledTimes(1);
    expect(destroy).not.toHaveBeenCalled();
    finish();
    await Promise.all([first, second]);
    expect(destroy).toHaveBeenCalledTimes(1);
  });

  it("keeps the window open on save failure and allows another close attempt", async () => {
    const flush = vi.fn(async () => {}).mockRejectedValueOnce(new Error("disk full"));
    const destroy = vi.fn(async () => {});
    const close = createWindowCloseHandler(flush, destroy);
    const event = { preventDefault: vi.fn() };
    await expect(close(event)).rejects.toThrow("disk full");
    expect(destroy).not.toHaveBeenCalled();
    await close(event);
    expect(destroy).toHaveBeenCalledTimes(1);
  });
});
