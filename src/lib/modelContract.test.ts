import { modelDisplay } from "./modelDisplay";
import { describe, expect, it } from "vitest";
import type { ModelGroup, ProviderId, ProviderInfo } from "./ipc";
import { currentAvailableModels, effectiveOptions, optionAxes, resolveVariant, selectableGroups } from "./models";
import { findInCatalog, formatSpec, idMatches, locateSpec, normalizeProviderId, optionsOf, paramsOf, parseSpec } from "./modelSpec";
import { reconcileModel } from "./modelReconcile";
import { resolveTaskProviderAndModel } from "./taskRunner";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";

type V = ModelGroup["variants"][number];
const v = (id: string, o: Partial<V> = {}): V => ({ id, effort: null, context: null, thinking: false, fast: false, ...o });
const group = (key: string, variants: V[], o: Partial<ModelGroup> = {}): ModelGroup => ({
  key,
  name: key,
  efforts: [],
  defaultEffort: null,
  contexts: [],
  thinking: variants.some((x) => x.thinking),
  fast: variants.some((x) => x.fast),
  tags: [],
  tiers: [],
  defaultTier: null,
  variants,
  ...o,
});
const provider = (id: ProviderId, modelGroups: ModelGroup[]): ProviderInfo =>
  ({ id, name: id, status: "READY", chat: true, modelGroups, models: [], features: [], guidance: [],
    ...(id === "REASONIX" ? { modelVerification: { state: "VALID", model: modelGroups[0].variants[0].id, verifiedAt: Math.floor(Date.now() / 1000), expiresAt: Math.floor(Date.now() / 1000) + 86400 } } : {}) }) as unknown as ProviderInfo;

const CLAUDE_EFFORTS = ["default", "low", "medium", "high", "xhigh", "max"];

/** Shapes the Rust catalog code produces for each CLI (see tokage-native providers/models). */
const PROVIDERS: ProviderInfo[] = [
  provider("CLAUDE_CODE", [
    group("opus", [v("opus"), v("opus[1m]", { context: "1m" })], { contexts: ["default", "1m"], efforts: CLAUDE_EFFORTS, defaultEffort: "default" }),
    group("sonnet", [v("sonnet"), v("sonnet[1m]", { context: "1m" })], { contexts: ["default", "1m"], efforts: CLAUDE_EFFORTS, defaultEffort: "default" }),
    group("haiku", [v("haiku")]),
    group("claude-fable-5-1", [v("claude-fable-5-1[1m]", { context: "1m" })], { efforts: CLAUDE_EFFORTS, defaultEffort: "default" }),
  ]),
  provider("CODEX_CLI", [
    group("gpt-5.6-sol", [v("gpt-5.6-sol")], { efforts: ["low", "medium", "high", "xhigh"], defaultEffort: "medium", fast: true }),
    group("gpt-5.4", [v("gpt-5.4")]),
    group("gpt-5.4-mini", [v("gpt-5.4-mini")]),
  ]),
  provider("CURSOR_CLI", [
    group(
      "gpt-5.3-codex",
      [
        v("gpt-5.3-codex-low", { effort: "low" }),
        v("gpt-5.3-codex-low-fast", { effort: "low", fast: true }),
        v("gpt-5.3-codex"),
        v("gpt-5.3-codex-fast", { fast: true }),
        v("gpt-5.3-codex-high", { effort: "high" }),
        v("gpt-5.3-codex-high-fast", { effort: "high", fast: true }),
        v("gpt-5.3-codex-xhigh", { effort: "xhigh" }),
      ],
      { efforts: ["default", "low", "high", "xhigh"], fast: true },
    ),
    group("claude-opus-5", [v("claude-opus-5-thinking-high", { effort: "high", thinking: true }), v("claude-opus-5-thinking-high-fast", { effort: "high", thinking: true, fast: true })], { thinking: true, fast: true }),
    group("claude-opus-5-5", [v("claude-opus-5-5-low", { effort: "low" }), v("claude-opus-5-5-medium", { effort: "medium" }), v("claude-opus-5-5-xhigh", { effort: "xhigh" })], { efforts: ["low", "medium", "xhigh"] }),
    group("auto", [v("auto")]),
  ]),
  provider("COPILOT_CLI", [
    group("auto", [v("auto")], { tiers: ["efficiency", "balance", "intelligence"], defaultTier: "balance" }),
    group("claude-opus-5.5", [v("claude-opus-5.5")]),
    group("claude-opus-5", [v("claude-opus-5")]),
  ]),
  provider("REASONIX", [group("deepseek/deepseek-flash", [v("deepseek/deepseek-flash")])]),
];

const enabled = Object.fromEntries(PROVIDERS.map((p) => [p.id, true]));
const trust = enabled;

/** Every combination of the options a model offers a choice for. */
function combos(m: ModelGroup) {
  const a = optionAxes(m);
  const out: Parameters<typeof resolveVariant>[1][] = [];
  const efforts = a.efforts.length > 1 ? a.efforts : [undefined];
  const contexts = a.contexts.length > 1 ? a.contexts : [undefined];
  const thinks = a.thinking ? [false, true] : [undefined];
  const fasts = a.fast ? [false, true] : [undefined];
  const tiers = a.tiers.length > 1 ? a.tiers : [undefined];
  for (const effort of efforts)
    for (const context of contexts)
      for (const thinking of thinks)
        for (const fast of fasts)
          for (const tier of tiers) out.push({ effort, context, thinking, fast, tier });
  return out;
}

describe("picker → resolver contract", () => {
  it("every spec the picker can emit resolves back to the same concrete id and flags", () => {
    let checked = 0;
    for (const p of PROVIDERS) {
      for (const m of p.modelGroups) {
        for (const o of combos(m)) {
          const r = resolveVariant(m, o);
          const spec = formatSpec(p.id, r.id);
          const params = paramsOf(r);
          const got = resolveTaskProviderAndModel(spec, PROVIDERS, enabled, trust, [], "off", undefined, undefined, params);
          const why = `${spec} ${JSON.stringify(o)}`;
          expect(got, why).not.toBeNull();
          expect(got!.provider, why).toBe(p.id);
          expect(got!.model, why).toBe(r.id);
          // Flags the id doesn't spell come back from the stored params.
          expect(got!.effort ?? null, why).toBe(r.effort ?? (m.variants.find((x) => x.id === r.id)?.effort ?? null));
          expect(got!.tier ?? null, why).toBe(r.tier);
          checked++;
        }
      }
    }
    expect(checked).toBeGreaterThan(30);
  });

  it("the picker's options reproduce a stored choice", () => {
    for (const p of PROVIDERS) {
      for (const m of p.modelGroups) {
        for (const o of combos(m)) {
          const r = resolveVariant(m, o);
          const variant = m.variants.find((x) => x.id === r.id)!;
          const back = resolveVariant(m, optionsOf(m, variant, paramsOf(r)));
          expect(back.id, `${p.id}:${r.id}`).toBe(r.id);
          expect(back.effort, `${p.id}:${r.id}`).toBe(r.effort);
        }
      }
    }
  });

  it("does not collapse a higher mode onto the shorter id (regression)", () => {
    for (const [spec, id] of [
      ["CURSOR_CLI:gpt-5.3-codex-high", "gpt-5.3-codex-high"],
      ["CURSOR_CLI:gpt-5.3-codex-xhigh", "gpt-5.3-codex-xhigh"],
      ["CURSOR_CLI:gpt-5.3-codex-high-fast", "gpt-5.3-codex-high-fast"],
      ["CLAUDE_CODE:opus[1m]", "opus[1m]"],
    ]) {
      expect(resolveTaskProviderAndModel(spec, PROVIDERS, enabled, trust)?.model).toBe(id);
    }
    // Not in the catalog verbatim: the closest mode of the same model, never another model.
    expect(resolveTaskProviderAndModel("CURSOR_CLI:gpt-5.3-codex-medium", PROVIDERS, enabled, trust)?.model).toBe("gpt-5.3-codex");
    expect(resolveTaskProviderAndModel("CODEX_CLI:gpt-5.4-mini", PROVIDERS, enabled, trust)?.model).toBe("gpt-5.4-mini");
    expect(resolveTaskProviderAndModel("CODEX_CLI:gpt-5.4-nano", PROVIDERS, enabled, trust)).toBeNull();
    expect(resolveTaskProviderAndModel("COPILOT_CLI:claude-opus-5.5-low", PROVIDERS, enabled, trust)?.model).toBe("claude-opus-5.5");
  });

  it("stored flags win only where the id has no say", () => {
    const codex = resolveTaskProviderAndModel("CODEX_CLI:gpt-5.6-sol", PROVIDERS, enabled, trust, [], "off", undefined, undefined, { effort: "high", fast: true });
    expect(codex).toMatchObject({ model: "gpt-5.6-sol", effort: "high", fast: true });
    const claude = resolveTaskProviderAndModel("CLAUDE_CODE:opus", PROVIDERS, enabled, trust, [], "off", undefined, undefined, { effort: "max" });
    expect(claude).toMatchObject({ model: "opus", effort: "max" });
    // The id's own effort is not overridden by a stale param.
    const cursor = resolveTaskProviderAndModel("CURSOR_CLI:gpt-5.3-codex-high", PROVIDERS, enabled, trust, [], "off", undefined, undefined, { effort: "low" });
    expect(cursor?.effort).toBe("high");
  });

  it("an unrecognised provider prefix is not stripped", () => {
    expect(parseSpec("REASONIX:deepseek/deepseek-flash")).toEqual({ provider: "REASONIX", model: "deepseek/deepseek-flash" });
    expect(parseSpec("someorg:model-1")).toEqual({ provider: null, model: "someorg:model-1" });
    expect(normalizeProviderId("anthropic")).toBe("CLAUDE_CODE");
    expect(normalizeProviderId("kiro-cli")).toBe("KIRO_CLI");
    const got = resolveTaskProviderAndModel("REASONIX:deepseek/deepseek-flash", PROVIDERS, enabled, trust);
    expect(got).toMatchObject({ provider: "REASONIX", model: "deepseek/deepseek-flash" });
  });

  it("matching is by model, then by mode suffix, never by substring", () => {
    expect(idMatches("gpt-5.4-mini", "gpt-5.4")).toBe(false);
    expect(idMatches("claude-opus-5-5-low", "claude-opus-5")).toBe(false);
    expect(idMatches("claude-opus-5-5-thinking-high", "claude-opus-5-5")).toBe(true);
    const codexLike = PROVIDERS[2].modelGroups;
    // A mode the catalog lacks is unresolved, not silently swapped for another effort (cost).
    expect(findInCatalog(codexLike, "claude-opus-5-5-high")).toBeNull();
    expect(findInCatalog(codexLike, "claude-opus-5-5-medium")?.variant.id).toBe("claude-opus-5-5-medium");
    expect(findInCatalog(codexLike, "opus")).toBeNull();
  });

  it("locates a spec inside the runnable groups only", () => {
    const groups = selectableGroups(PROVIDERS, { ...enabled, CURSOR_CLI: false }, trust, "direct", [], "off", undefined, undefined, {});
    expect(locateSpec(groups, "CURSOR_CLI:gpt-5.3-codex-high")).toBeNull();
    expect(locateSpec(groups, "CODEX_CLI:gpt-5.6-sol")?.ref).toEqual({ provider: "CODEX_CLI", model: "gpt-5.6-sol" });
  });
});

describe("catalog sources", () => {
  it("custom model ids join the catalog and resolve", () => {
    const got = resolveTaskProviderAndModel(
      "CLAUDE_CODE:claude-opus-5-5",
      PROVIDERS, enabled, trust, [], "off", undefined, undefined, null,
      { CLAUDE_CODE: ["claude-opus-5-5"] },
    );
    expect(got).toMatchObject({ provider: "CLAUDE_CODE", model: "claude-opus-5-5" });
    expect(resolveTaskProviderAndModel("CLAUDE_CODE:claude-opus-5-5", PROVIDERS, enabled, trust)).toBeNull();
  });

  it("the allocator is told exactly what can run", () => {
    const before = { p: useProviders.getState().providers, s: useSettings.getState() };
    useProviders.setState({ providers: PROVIDERS });
    useSettings.setState({ providerEnabled: { ...enabled, REASONIX: false }, providerTrust: trust, blockedModels: [] });
    try {
      const avail = currentAvailableModels()!;
      const has = (provider: string, id: string) => avail.some((a) => a.provider === provider && a.cliModelArg === id);
      expect(has("CURSOR_CLI", "gpt-5.3-codex-high")).toBe(true);
      expect(has("CLAUDE_CODE", "opus[1m]")).toBe(true);
      expect(has("REASONIX", "deepseek/deepseek-flash")).toBe(false);
    } finally {
      useProviders.setState({ providers: before.p });
      useSettings.setState({ providerEnabled: before.s.providerEnabled, providerTrust: before.s.providerTrust });
    }
  });

  it("Claude effort is a flag, not part of the id", () => {
    const opus = PROVIDERS[0].modelGroups[0];
    expect(effectiveOptions(opus, {}).effort).toBe("default");
    expect(resolveVariant(opus, { effort: "high" })).toMatchObject({ id: "opus", effort: "high" });
    expect(resolveVariant(opus, { effort: "default", context: "1m" })).toMatchObject({ id: "opus[1m]", effort: null });
  });
});

describe("requested vs reported model", () => {
  it("tells aliases and routers from a real swap", () => {
    expect(reconcileModel("CLAUDE_CODE", "opus", "claude-opus-5-5").kind).toBe("resolved");
    expect(reconcileModel("CLAUDE_CODE", "opus[1m]", "claude-opus-5-5").kind).toBe("resolved");
    expect(reconcileModel("CURSOR_CLI", "auto", "gpt-5.6-luna-high").kind).toBe("resolved");
    expect(reconcileModel("CODEX_CLI", "gpt-5.6-sol", "gpt-5.6-sol").kind).toBe("match");
    expect(reconcileModel("COPILOT_CLI", "claude-opus-5.5", "claude-opus-5-5").kind).toBe("match");
    expect(reconcileModel("CURSOR_CLI", "gpt-5.3-codex-high", "gpt-5.3-codex").kind).toBe("match");
    expect(reconcileModel("AIDER", "deepseek/deepseek-v4-pro", "deepseek-v4-pro").kind).toBe("match");
    expect(reconcileModel("CODEX_CLI", "gpt-5.6-sol", "gpt-5.6-luna")).toMatchObject({ kind: "mismatch" });
    expect(reconcileModel("CLAUDE_CODE", "claude-opus-5", "claude-opus-5-5").kind).toBe("mismatch");
    expect(reconcileModel("CLAUDE_CODE", "claude-opus-5-5", "claude-opus-5-5-20251001").kind).toBe("match");
  });
});


describe("shared model presentation", () => {
  it("shows the same context label for picker options and concrete runtime IDs without changing the assignment", () => {
    const assignment = { provider: "CLAUDE_CODE", requestedModel: "opus[1m]", modelParams: { effort: "high" } };
    expect(modelDisplay(assignment, PROVIDERS).label).toBe("Claude · Opus[1M] · High");
    expect(modelDisplay({ provider: "CLAUDE_CODE", requestedModel: "opus", details: ["High", "1M"] }, PROVIDERS).label).toBe("Claude · Opus[1M] · High");
    expect(assignment.requestedModel).toBe("opus[1m]");
    expect(modelDisplay({ provider: "CLAUDE_CODE", requestedModel: "claude-fable-5-1[1m]" }, PROVIDERS).label).toBe("Claude · Fable 5.1[1M]");
    expect(modelDisplay({ provider: "CURSOR_CLI", requestedModel: "claude-opus-5-5", details: [] }, PROVIDERS).label).toBe("Cursor · Claude Opus 5.5");
  });
});
