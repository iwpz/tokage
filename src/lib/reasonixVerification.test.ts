import { afterEach, expect, it, vi } from "vitest";
import type { ModelGroup, ProviderInfo } from "./ipc";
import { runnable, selectableGroups } from "./models";

const group = (id: string): ModelGroup => ({ key: id, name: id, efforts: [], defaultEffort: null, contexts: [], thinking: false, fast: false, tags: [], tiers: [], defaultTier: null,
  variants: [{ id, effort: null, context: null, thinking: false, fast: false }] });
function provider(): ProviderInfo {
  return { id: "REASONIX", name: "Reasonix", executable: "reasonix", featured: true, status: "READY", version: "2.31.0", path: "/reasonix", authMethod: "api_key",
    models: [], modelGroups: [group("deepseek/flash"), group("deepseek/pro")], modelsSource: "CLI", account: null, guidance: [], installMethods: [], trustArgs: [], chat: true, adapter: null, directLimit: null,
    modelVerification: { state: "VALID", model: "deepseek/flash", verifiedAt: 1, expiresAt: 86401 } };
}
afterEach(() => vi.useRealTimers());
it("excludes other models and expires cached Ready at the exact evidence boundary", () => {
  vi.useFakeTimers();
  vi.setSystemTime(86400_000);
  const p = provider();
  expect(runnable(p)).toBe(true);
  expect(selectableGroups([p], {}, { REASONIX: true }, undefined, [], "off", undefined, undefined, {})[0].models.map((m) => m.key)).toEqual(["deepseek/flash"]);
  vi.setSystemTime(86401_000);
  expect(runnable(p)).toBe(false);
  expect(selectableGroups([p], {}, { REASONIX: true })).toEqual([]);
});
it("never treats a wallet or missing/invalid verification as model authentication", () => {
  vi.useFakeTimers(); vi.setSystemTime(1000);
  for (const state of ["EXPIRED", "REQUIRED", "SOURCE_CHANGED", "REJECTED"] as const) {
    const p = provider(); p.modelVerification!.state = state;
    p.account = { billing: "PAY_AS_YOU_GO", official: true, endpoint: null, plan: null, balance: "CNY 12", usage: null };
    expect(runnable(p)).toBe(false);
  }
  const p = provider(); delete p.modelVerification;
  expect(runnable(p)).toBe(false);
});
