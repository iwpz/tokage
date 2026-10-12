import { afterEach, describe, expect, it } from "vitest";
import i18n from "../i18n";
import { lm, lmRef, lmText, renderLogMsg } from "./logMsg";

describe("logMsg", () => {
  afterEach(() => void i18n.changeLanguage("en"));

  it("renders the same stored message in the active language", async () => {
    const msg = lm("taskLog.merge.merged", { mergedBranch: "feat/x" });
    await i18n.changeLanguage("en");
    expect(renderLogMsg(msg)).toBe("[Merge] Merged feat/x");
    await i18n.changeLanguage("zh");
    expect(renderLogMsg(msg)).toBe("[合并] 已合入 feat/x");
  });

  it("resolves nested refs and encoded string params", async () => {
    const inner = lm("taskLog.delivery.mergeConflict");
    const msg = lm("taskLog.delivery.mergeFailed", { msg: inner });
    expect(renderLogMsg(msg)).toContain("Merge conflict");
    const step = lm("taskLog.ladder.step", { n: 2, nextStep: lmRef("taskLog.step.reassigned"), currentStep: lmRef("taskLog.step.inWindowRepair") });
    expect(renderLogMsg(step)).toContain("Reassign");
  });

  it("passes plain strings through and lmText translates immediately", () => {
    expect(renderLogMsg("raw CLI output")).toBe("raw CLI output");
    expect(lmText("taskLog.none")).toBe("(none)");
  });
});
