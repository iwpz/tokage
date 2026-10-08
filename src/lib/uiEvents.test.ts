import { afterEach, describe, expect, it } from "vitest";
import { drainUiEventsNow, pushUiEvent, subscribeUiEvents } from "./uiEvents";

afterEach(() => {
  drainUiEventsNow();
});

describe("uiEvents batcher", () => {
  it("batches events within 50ms", async () => {
    const batches: number[] = [];
    const unsub = subscribeUiEvents((b) => batches.push(b.length));
    pushUiEvent({ type: "notice", level: "info", message: "a" });
    pushUiEvent({ type: "notice", level: "info", message: "b" });
    await new Promise((r) => setTimeout(r, 60));
    expect(batches).toEqual([2]);
    unsub();
  });

  it("drainUiEventsNow clears buffer without notifying", () => {
    pushUiEvent({ type: "tree_changed", sessionId: "s" });
    expect(drainUiEventsNow()).toHaveLength(1);
    expect(drainUiEventsNow()).toHaveLength(0);
  });
});
