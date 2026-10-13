import { describe, expect, it } from "vitest";
import { canResumeNativeThread } from "./nativeSessionResume";
import type { ModelRef } from "./models";

const cursor: ModelRef = { provider: "CURSOR_CLI", model: "auto" };
const codex: ModelRef = { provider: "CODEX_CLI", model: "gpt-6-luna" };

describe("Cursor native session mode isolation", () => {
  it("resumes Cursor only within the same mode", () => {
    expect(canResumeNativeThread({ ref: cursor, id: "native", mode: "plan" }, cursor, "plan")).toBe(true);
    expect(canResumeNativeThread({ ref: cursor, id: "native", mode: "plan" }, cursor, "direct")).toBe(false);
    expect(canResumeNativeThread({ ref: cursor, id: "native", mode: "direct" }, cursor, "plan")).toBe(false);
    expect(canResumeNativeThread({ ref: cursor, id: "native", mode: "direct" }, cursor, "direct")).toBe(true);
  });
  it("restarts legacy Cursor threads whose CLI mode is unknown", () => {
    expect(canResumeNativeThread({ ref: cursor, id: "legacy" }, cursor, "direct")).toBe(false);
    expect(canResumeNativeThread({ ref: cursor, id: "legacy" }, cursor, "plan")).toBe(false);
  });
  it("preserves other providers and model mismatch rules", () => {
    expect(canResumeNativeThread({ ref: codex, id: "native" }, codex, "direct")).toBe(true);
    expect(canResumeNativeThread({ ref: codex, id: "native", mode: "plan" }, codex, "direct")).toBe(true);
    expect(canResumeNativeThread({ ref: cursor, id: "native", mode: "direct" }, { ...cursor, model: "other" }, "direct")).toBe(false);
    expect(canResumeNativeThread(null, cursor, "direct")).toBe(false);
  });
});
