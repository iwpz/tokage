import { describe, expect, it } from "vitest";
import { equalOrderedSnapshots } from "./historySnapshot";

describe("equalOrderedSnapshots", () => {
  it("compares ordered values and own keys independent of insertion order", () => {
    expect(equalOrderedSnapshots([{ id: 1, meta: "{}" }], [{ meta: "{}", id: 1 }])).toBe(true);
    expect(equalOrderedSnapshots([{ id: 1 }], [{ id: 1, future: undefined }])).toBe(false);
    expect(equalOrderedSnapshots([{ id: 1, meta: "{}" }], [{ id: 1, meta: "{ }" }])).toBe(false);
    expect(equalOrderedSnapshots([{ id: 1 }, { id: 2 }], [{ id: 2 }, { id: 1 }])).toBe(false);
    expect(equalOrderedSnapshots([{ id: 1 }], [])).toBe(false);
  });
});
