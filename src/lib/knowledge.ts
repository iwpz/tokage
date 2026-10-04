/**
 * Knowledge pack parsing (docs/04 knowledge-pack.md). Only the parts the settings page shows
 * are read; everything is validated because a pack loaded from a custom URL is untrusted.
 */

export const SUPPORTED_SCHEMA = 1;
const MAX_MODELS = 1000;

/** One CLI's listing of a model: only what differs by channel (docs/04 knowledge-pack §3). */
export interface PackOffering {
  provider: string;
  cliModelArg: string;
  /** CLI-imposed limits; `null` means the model's own value applies. */
  contextWindow: number | null;
  effectiveContext: number | null;
  creditMultiplier: number | null;
  /** `false` when the argument wasn't checked against the CLI's real model list. */
  verified: boolean;
  billing: string | null;
}

export interface PackModel {
  modelId: string;
  displayName: string;
  vendor: string | null;
  roles: string[];
  /** Domain → score (1–5). */
  skills: Record<string, number>;
  price: { input: number; output: number } | null;
  contextWindow: number | null;
  effectiveContext: number | null;
  speed: number | null;
  tags: string[];
  description: string | null;
  priceEstimated: boolean;
  skillsEstimated: boolean;
  offerings: PackOffering[];
}

export interface KnowledgePack {
  schemaVersion: number;
  packVersion: string;
  domains: string[];
  models: PackModel[];
}

export interface ParseResult {
  pack: KnowledgePack;
  /** Non-fatal problems: skipped entries, clamped scores, unknown domains or models. */
  warnings: string[];
}

export class PackError extends Error {}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const str = (v: unknown, max = 200): string | null => (typeof v === "string" && v.trim() ? v.trim().slice(0, max) : null);
const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);
const strList = (v: unknown, max = 32): string[] =>
  Array.isArray(v) ? v.map((x) => str(x, 64)).filter((x): x is string => !!x).slice(0, max) : [];

export function parsePack(text: string): ParseResult {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    throw new PackError(`JSON: ${(e as Error).message}`);
  }
  if (!isObj(raw)) throw new PackError("Top level must be a JSON object");
  const schemaVersion = num(raw.schema_version);
  if (schemaVersion !== SUPPORTED_SCHEMA) {
    throw new PackError(`Unsupported schema_version: ${String(raw.schema_version)} (expected ${SUPPORTED_SCHEMA})`);
  }
  if (!isObj(raw.models)) throw new PackError("`models` must be an object keyed by model id");

  const warnings: string[] = [];
  const domains = strList(raw.domains, 64);
  const known = new Set(domains);
  const byId = new Map<string, PackModel>();

  const entries = Object.entries(raw.models);
  for (const [rawId, m] of entries.slice(0, MAX_MODELS)) {
    const modelId = str(rawId);
    if (!modelId) continue;
    if (!isObj(m)) {
      warnings.push(`models.${modelId}: not an object, skipped`);
      continue;
    }
    const skills: Record<string, number> = {};
    if (isObj(m.skills)) {
      for (const [d, v] of Object.entries(m.skills).slice(0, 64)) {
        const n = num(v);
        if (n === null) {
          warnings.push(`${modelId}: skills.${d} is not a number`);
          continue;
        }
        if (n < 1 || n > 5) warnings.push(`${modelId}: skills.${d} = ${n} clamped to 1–5`);
        if (known.size > 0 && !known.has(d)) warnings.push(`${modelId}: unknown domain "${d}"`);
        skills[d.slice(0, 64)] = Math.min(5, Math.max(1, n));
      }
    } else {
      warnings.push(`${modelId}: no skills`);
    }

    const p = isObj(m.price_per_mtok) ? m.price_per_mtok : null;
    const input = num(p?.input), output = num(p?.output);
    byId.set(modelId, {
      modelId,
      displayName: str(m.display_name) ?? modelId,
      vendor: str(m.vendor, 64),
      roles: strList(m.roles),
      skills,
      price: input !== null && output !== null && input >= 0 && output >= 0 ? { input, output } : null,
      contextWindow: positive(m.context_window),
      effectiveContext: positive(m.effective_context),
      speed: num(m.speed),
      tags: strList(m.tags),
      description: str(m.description, 1000),
      priceEstimated: m.price_estimated === true,
      skillsEstimated: m.skills_estimated === true,
      offerings: [],
    });
  }
  if (entries.length > MAX_MODELS) warnings.push(`Only the first ${MAX_MODELS} models were read`);

  // Attach each CLI's listings to the shared model entry.
  if (isObj(raw.providers)) {
    for (const [rawProvider, prov] of Object.entries(raw.providers).slice(0, 128)) {
      const provider = str(rawProvider, 64);
      if (!provider || !isObj(prov) || prov.models === undefined) continue;
      if (!Array.isArray(prov.models)) {
        warnings.push(`providers.${provider}.models: not an array, skipped`);
        continue;
      }
      const billing = str(prov.billing, 32);
      const seen = new Set<string>();
      prov.models.slice(0, MAX_MODELS).forEach((o, i) => {
        const at = `providers.${provider}.models[${i}]`;
        if (!isObj(o)) return warnings.push(`${at}: not an object, skipped`);
        const id = str(o.model);
        const model = id ? byId.get(id) : undefined;
        if (!id || !model) return warnings.push(`${at}: unknown model "${String(o.model)}", skipped`);
        if (seen.has(id)) return warnings.push(`${at}: duplicate ${provider}:${id}, skipped`);
        seen.add(id);
        model.offerings.push({
          provider,
          cliModelArg: str(o.cli_model_arg) ?? id,
          contextWindow: positive(o.context_window),
          effectiveContext: positive(o.effective_context),
          creditMultiplier: num(o.credit_multiplier),
          verified: o.cli_model_arg_verified !== false,
          billing,
        });
      });
    }
  }

  const models = [...byId.values()];
  return {
    pack: {
      schemaVersion,
      packVersion: str(raw.pack_version, 64) ?? "unknown",
      // Fall back to the domains models actually use when the pack lists none.
      domains: domains.length > 0 ? domains : [...new Set(models.flatMap((m) => Object.keys(m.skills)))],
      models,
    },
    warnings,
  };
}

function positive(v: unknown): number | null {
  const n = num(v);
  return n !== null && n > 0 ? Math.round(n) : null;
}
