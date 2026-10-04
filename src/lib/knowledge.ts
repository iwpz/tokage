/**
 * Knowledge pack parsing (docs/04 knowledge-pack.md). Only the parts the settings page shows
 * are read; everything is validated because a pack loaded from a custom URL is untrusted.
 */

export const SUPPORTED_SCHEMA = 1;
const MAX_MODELS = 1000;

export interface PackModel {
  modelId: string;
  provider: string;
  displayName: string;
  cliModelArg: string | null;
  roles: string[];
  /** Domain → score (1–5). */
  skills: Record<string, number>;
  price: { input: number; output: number } | null;
  contextWindow: number | null;
  effectiveContext: number | null;
  speed: number | null;
  tags: string[];
  description: string | null;
}

export interface KnowledgePack {
  schemaVersion: number;
  packVersion: string;
  domains: string[];
  models: PackModel[];
}

export interface ParseResult {
  pack: KnowledgePack;
  /** Non-fatal problems: skipped models, clamped scores, unknown domains. */
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
  if (!Array.isArray(raw.models)) throw new PackError("`models` must be an array");

  const warnings: string[] = [];
  const domains = strList(raw.domains, 64);
  const known = new Set(domains);
  const models: PackModel[] = [];
  const seen = new Set<string>();

  raw.models.slice(0, MAX_MODELS).forEach((m, i) => {
    const at = `models[${i}]`;
    if (!isObj(m)) return warnings.push(`${at}: not an object, skipped`);
    const modelId = str(m.model_id);
    const provider = str(m.provider, 64);
    if (!modelId || !provider) return warnings.push(`${at}: missing model_id or provider, skipped`);
    const key = `${provider}:${modelId}`;
    if (seen.has(key)) return warnings.push(`${at}: duplicate ${key}, skipped`);
    seen.add(key);

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
    models.push({
      modelId,
      provider,
      displayName: str(m.display_name) ?? modelId,
      cliModelArg: str(m.cli_model_arg),
      roles: strList(m.roles),
      skills,
      price: input !== null && output !== null && input >= 0 && output >= 0 ? { input, output } : null,
      contextWindow: positive(m.context_window),
      effectiveContext: positive(m.effective_context),
      speed: num(m.speed),
      tags: strList(m.tags),
      description: str(m.description, 1000),
    });
  });
  if (raw.models.length > MAX_MODELS) warnings.push(`Only the first ${MAX_MODELS} models were read`);

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
