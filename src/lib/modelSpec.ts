/**
 * One place that turns "what the user picked" into "what the CLI is asked to run".
 *
 * A task's model is stored as `PROVIDER:variantId` (`assigned_model`) plus the parameters an id
 * can't carry (`model_params`: effort / fast / tier for CLIs that take them as flags). The id
 * is always a concrete catalog id the CLI accepts, never a pack spelling or a guess.
 */
import type { ModelGroup as CatalogModel, ModelParams, ProviderId } from "./ipc";
import type { ModelGroup as ProviderModels, ModelOptions, ModelRef } from "./models";
import { effectiveOptions, resolveVariant } from "./models";

type Variant = CatalogModel["variants"][number];

export const PROVIDER_IDS: ProviderId[] = [
  "CLAUDE_CODE", "CODEX_CLI", "CURSOR_CLI", "ANTIGRAVITY_CLI", "COPILOT_CLI", "AIDER", "OPENCODE",
  "GROK_CLI", "KIRO_CLI", "QWEN_CODE", "KIMI_CODE", "QODER_CLI", "AMP", "CRUSH", "GOOSE", "CLINE",
  "DROID", "AUGGIE", "MUSE_CODE", "TRAE_CLI", "REASONIX",
];

const PROVIDER_ALIASES: Record<string, ProviderId> = {
  ANTHROPIC: "CLAUDE_CODE",
  CLAUDE: "CLAUDE_CODE",
  OPENAI: "CODEX_CLI",
  CODEX: "CODEX_CLI",
  CURSOR: "CURSOR_CLI",
  GITHUB: "COPILOT_CLI",
  COPILOT: "COPILOT_CLI",
  GOOGLE: "ANTIGRAVITY_CLI",
  ANTIGRAVITY: "ANTIGRAVITY_CLI",
  AGY: "ANTIGRAVITY_CLI",
  GROK: "GROK_CLI",
  KIRO: "KIRO_CLI",
  QWEN: "QWEN_CODE",
  KIMI: "KIMI_CODE",
  QODER: "QODER_CLI",
  TRAE: "TRAE_CLI",
  MUSE: "MUSE_CODE",
};

/** `ProviderId`, its short form, or a vendor name → the ProviderId it means. */
export function normalizeProviderId(token: string): ProviderId | null {
  const s = token.trim().toUpperCase().replace(/[\s-]+/g, "_");
  if ((PROVIDER_IDS as string[]).includes(s)) return s as ProviderId;
  return PROVIDER_ALIASES[s] ?? null;
}

/** `PROVIDER:model`. Only a recognised provider prefix is split off, so ids that contain a
 * colon themselves are not cut in half. */
export function parseSpec(spec: string): { provider: ProviderId | null; model: string } {
  const s = spec.trim();
  const i = s.indexOf(":");
  if (i > 0) {
    const provider = normalizeProviderId(s.slice(0, i));
    if (provider) return { provider, model: s.slice(i + 1).trim() };
  }
  return { provider: null, model: s };
}

export const formatSpec = (provider: ProviderId, variantId: string) => `${provider}:${variantId}`;

const norm = (s: string) => s.trim().toLowerCase().replace(/\./g, "-");

/** Segments that only select a mode of the same model, never a different model. */
const MODE_SEGMENTS = new Set([
  "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra", "thinking", "fast", "extra", "default",
]);
const isModeSegment = (seg: string) => MODE_SEGMENTS.has(seg) || /^\d+[km]$/.test(seg);

/**
 * `live` is `key` or `key` plus mode suffixes (`-high`, `-thinking-high-fast`, `[1m]`).
 * `gpt-5.4-mini` is not a mode of `gpt-5.4`; `claude-opus-5-5-low` is not a mode of
 * `claude-opus-5`. Mirrors `id_matches` in tokage-matching.
 */
export function idMatches(live: string, key: string): boolean {
  const l = norm(live);
  const k = norm(key);
  if (!k || !l.startsWith(k)) return false;
  let rest = l.slice(k.length);
  if (!rest) return true;
  const b = rest.indexOf("[");
  if (b >= 0 && rest.endsWith("]")) rest = rest.slice(0, b);
  if (!rest) return true;
  if (!rest.startsWith("-")) return false;
  return rest.slice(1).split("-").every(isModeSegment);
}

export interface CatalogHit {
  model: CatalogModel;
  variant: Variant;
  /** The target named the model but not a concrete variant, so the model's default was used. */
  byModel: boolean;
}

/**
 * Finds a model + variant for a target string, strictest rule first:
 *   1. a variant id, exactly;
 *   2. the model key or display name, exactly (then the model's default variant);
 *   3. a variant id the target only extends by mode suffixes (longest id wins), which keeps
 *      `gpt-5.3-codex-high` from collapsing onto the shorter `gpt-5.3-codex`.
 * Never a bare substring match.
 */
export function findInCatalog(models: CatalogModel[], target: string): CatalogHit | null {
  const t = target.trim().toLowerCase();
  if (!t) return null;
  for (const m of models) {
    const v = m.variants.find((x) => x.id.toLowerCase() === t);
    if (v) return { model: m, variant: v, byModel: false };
  }
  for (const m of models) {
    if (m.key.toLowerCase() === t || m.name.toLowerCase() === t) {
      const id = resolveVariant(m, {}).id;
      return { model: m, variant: m.variants.find((v) => v.id === id) ?? m.variants[0], byModel: true };
    }
  }
  let best: { hit: CatalogHit; len: number } | null = null;
  for (const m of models) {
    for (const v of m.variants) {
      if (idMatches(t, v.id) && (!best || v.id.length > best.len)) {
        best = { hit: { model: m, variant: v, byModel: false }, len: v.id.length };
      }
    }
  }
  return best?.hit ?? null;
}

/** Parameters worth storing: flags the id doesn't carry. `null` when there are none. */
export function paramsOf(r: { effort: string | null; fast: boolean; tier: string | null }): ModelParams | null {
  const p: ModelParams = {};
  if (r.effort) p.effort = r.effort;
  if (r.fast) p.fast = true;
  if (r.tier) p.tier = r.tier;
  return Object.keys(p).length ? p : null;
}

/** The picker options that reproduce a stored `variantId` + `params`. */
export function optionsOf(model: CatalogModel, variant: Variant, params?: ModelParams | null): ModelOptions {
  const o: ModelOptions = {};
  const effort = variant.effort ?? params?.effort ?? undefined;
  if (effort) o.effort = effort;
  if (variant.context) o.context = variant.context;
  if (variant.thinking) o.thinking = true;
  if (variant.fast || params?.fast) o.fast = true;
  if (params?.tier) o.tier = params.tier;
  // Only keep what the model actually offers a choice for.
  const e = effectiveOptions(model, o);
  return {
    ...(model.efforts.length > 1 ? { effort: e.effort } : {}),
    ...(model.contexts.length > 1 ? { context: e.context } : {}),
    ...(o.thinking !== undefined ? { thinking: o.thinking } : {}),
    ...(o.fast !== undefined ? { fast: o.fast } : {}),
    ...(model.tiers.length > 1 ? { tier: e.tier } : {}),
  };
}

export interface ResolvedPick {
  ref: ModelRef;
  model: CatalogModel;
  variant: Variant;
}

/** Locates a stored spec in the runnable groups. Null when it isn't runnable right now. */
export function locateSpec(groups: ProviderModels[], spec: string | null | undefined): ResolvedPick | null {
  if (!spec) return null;
  const { provider, model } = parseSpec(spec);
  for (const g of groups) {
    if (provider && g.provider.id !== provider) continue;
    const hit = findInCatalog(g.models, model);
    if (hit) return { ref: { provider: g.provider.id, model: hit.model.key }, model: hit.model, variant: hit.variant };
    // An unprefixed target is searched across providers in catalog order.
  }
  return null;
}
