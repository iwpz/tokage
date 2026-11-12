import { describe, it, expect } from "vitest";
import { triageMainSessionMessage } from "../../lib/ipc";

describe("Frontend Context Lifecycle Engine (P8.m1)", () => {

  it("routes appended messages via triage", async () => {
    const route = await triageMainSessionMessage({
      message: "请问这个概念是什么？",
      activeTasks: [],
      currentTimeMs: 1000,
    });
    expect(route.type).toBe("DIRECT_DISPATCH");
  });

});
