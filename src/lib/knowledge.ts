/**
 * Knowledge pack parsing (docs/04 knowledge-pack.md).
 * Validates and normalizes models, providers, success model, cost model parameters,
 * and usage profiles.
 */

export const SUPPORTED_SCHEMA = 1;
const MAX_MODELS = 1000;

/** One CLI's listing of a model: only what differs by channel (docs/04 knowledge-pack §2, §3). */
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
  skillsPatch?: Record<string, number>;
}

export interface CacheRates {
  readMultiplier: number;
  writeMultiplier: number;
}

export interface LongContextPricing {
  threshold: number;
  inputMultiplier: number;
  outputMultiplier: number;
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
  released: string | null;
  maxOutput: number | null;
  cacheRates: CacheRates | null;
  longContextPricing: LongContextPricing | null;
  priceEstimated: boolean;
  skillsEstimated: boolean;
  offerings: PackOffering[];
}

export interface ProviderCache {
  mode: string;
  ttlSeconds?: number;
  readMultiplier?: number;
  writeMultiplier?: number;
  sharedBaseline?: boolean;
}

export interface ProviderContext {
  cliAutoCompactRatio?: number;
  compact?: string;
  compactOutputRatio?: number;
}

export interface PackProvider {
  provider: string;
  billing: string;
  cache?: ProviderCache;
  context?: ProviderContext;
  concurrencyHint?: number;
  envAllow?: string[];
  proxy?: Record<string, unknown>;
  quota?: Record<string, unknown>;
  models: PackOffering[];
}

export interface SuccessModel {
  a: number;
  b: number;
  priorStrength: number;
  req: Record<string, Record<string, number>>;
  plannerMinSkill?: number;
  pMin?: { default: number; difficulty3: number };
}

export interface UsageProfileEntry {
  turns: number;
  callsPerTurn: number;
  growthPerTurn: number;
  outputPerTurn: number;
}

export interface KnowledgePack {
  schemaVersion: number;
  packVersion: string;
  generatedAt: string | null;
  domains: string[];
  models: PackModel[];
  providers: Record<string, PackProvider>;
  successModel: SuccessModel | null;
  usageProfiles: Record<string, Record<string, UsageProfileEntry>>;
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

  // Parse models
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

    let cacheRates: CacheRates | null = null;
    if (isObj(m.cache_rates)) {
      const r = num(m.cache_rates.read_multiplier);
      const w = num(m.cache_rates.write_multiplier);
      if (r !== null && w !== null) {
        cacheRates = { readMultiplier: r, writeMultiplier: w };
      }
    }

    let longContextPricing: LongContextPricing | null = null;
    if (isObj(m.long_context_pricing)) {
      const thresh = positive(m.long_context_pricing.threshold);
      const inM = num(m.long_context_pricing.input_multiplier);
      const outM = num(m.long_context_pricing.output_multiplier);
      if (thresh !== null && inM !== null && outM !== null) {
        longContextPricing = { threshold: thresh, inputMultiplier: inM, outputMultiplier: outM };
      }
    }

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
      released: str(m.released, 32),
      maxOutput: positive(m.max_output),
      cacheRates,
      longContextPricing,
      priceEstimated: m.price_estimated === true,
      skillsEstimated: m.skills_estimated === true,
      offerings: [],
    });
  }
  if (entries.length > MAX_MODELS) warnings.push(`Only the first ${MAX_MODELS} models were read`);

  // Parse providers
  const providers: Record<string, PackProvider> = {};
  if (isObj(raw.providers)) {
    for (const [rawProvider, prov] of Object.entries(raw.providers).slice(0, 128)) {
      const provider = str(rawProvider, 64);
      if (!provider || !isObj(prov) || prov.models === undefined) continue;
      if (!Array.isArray(prov.models)) {
        warnings.push(`providers.${provider}.models: not an array, skipped`);
        continue;
      }
      const billing = str(prov.billing, 32) ?? "UNKNOWN";
      const packOfferings: PackOffering[] = [];
      const seen = new Set<string>();

      prov.models.slice(0, MAX_MODELS).forEach((o, i) => {
        const at = `providers.${provider}.models[${i}]`;
        if (!isObj(o)) return warnings.push(`${at}: not an object, skipped`);
        const id = str(o.model);
        const model = id ? byId.get(id) : undefined;
        if (!id || !model) return warnings.push(`${at}: unknown model "${String(o.model)}", skipped`);
        if (seen.has(id)) return warnings.push(`${at}: duplicate ${provider}:${id}, skipped`);
        seen.add(id);

        let skillsPatch: Record<string, number> | undefined;
        if (isObj(o.skills)) {
          skillsPatch = {};
          for (const [d, v] of Object.entries(o.skills)) {
            const n = num(v);
            if (n !== null) skillsPatch[d] = Math.min(5, Math.max(1, n));
          }
        }

        const offering: PackOffering = {
          provider,
          cliModelArg: str(o.cli_model_arg) ?? id,
          contextWindow: positive(o.context_window),
          effectiveContext: positive(o.effective_context),
          creditMultiplier: num(o.credit_multiplier),
          verified: o.cli_model_arg_verified !== false,
          billing,
          skillsPatch,
        };
        model.offerings.push(offering);
        packOfferings.push(offering);
      });

      let cache: ProviderCache | undefined;
      if (isObj(prov.cache) && typeof prov.cache.mode === "string") {
        cache = {
          mode: prov.cache.mode,
          ttlSeconds: positive(prov.cache.ttl_seconds) ?? undefined,
          readMultiplier: num(prov.cache.read_multiplier) ?? undefined,
          writeMultiplier: num(prov.cache.write_multiplier) ?? undefined,
          sharedBaseline: typeof prov.cache.shared_baseline === "boolean" ? prov.cache.shared_baseline : undefined,
        };
      }

      let context: ProviderContext | undefined;
      if (isObj(prov.context)) {
        context = {
          cliAutoCompactRatio: num(prov.context.cli_auto_compact_ratio) ?? undefined,
          compact: str(prov.context.compact, 32) ?? undefined,
          compactOutputRatio: num(prov.context.compact_output_ratio) ?? undefined,
        };
      }

      providers[provider] = {
        provider,
        billing,
        cache,
        context,
        concurrencyHint: positive(prov.concurrency_hint) ?? undefined,
        envAllow: strList(prov.env_allow),
        proxy: isObj(prov.proxy) ? prov.proxy : undefined,
        quota: isObj(prov.quota) ? prov.quota : undefined,
        models: packOfferings,
      };
    }
  }

  // Parse success_model (requirement table)
  let successModel: SuccessModel | null = null;
  if (isObj(raw.success_model)) {
    const a = num(raw.success_model.a);
    const b = num(raw.success_model.b);
    const priorStrength = num(raw.success_model.prior_strength);
    if (a !== null && b !== null && priorStrength !== null && isObj(raw.success_model.req)) {
      const req: Record<string, Record<string, number>> = {};
      for (const [diff, sizes] of Object.entries(raw.success_model.req)) {
        if (isObj(sizes)) {
          req[diff] = {};
          for (const [sz, sc] of Object.entries(sizes)) {
            const scNum = num(sc);
            if (scNum !== null) req[diff][sz] = scNum;
          }
        }
      }
      let pMin: { default: number; difficulty3: number } | undefined;
      if (isObj(raw.success_model.p_min)) {
        const def = num(raw.success_model.p_min.default);
        const d3 = num(raw.success_model.p_min.difficulty_3);
        if (def !== null && d3 !== null) {
          pMin = { default: def, difficulty3: d3 };
        }
      }
      successModel = {
        a,
        b,
        priorStrength,
        req,
        plannerMinSkill: num(raw.success_model.planner_min_skill) ?? 5,
        pMin,
      };
    } else {
      warnings.push("success_model missing required numeric fields (a, b, prior_strength, req)");
    }
  }

  // Parse usage_profiles
  const usageProfiles: Record<string, Record<string, UsageProfileEntry>> = {};
  if (isObj(raw.usage_profiles)) {
    for (const [d, sizes] of Object.entries(raw.usage_profiles)) {
      if (isObj(sizes)) {
        usageProfiles[d] = {};
        for (const [sz, prof] of Object.entries(sizes)) {
          if (isObj(prof)) {
            const turns = positive(prof.turns);
            const calls = positive(prof.calls_per_turn);
            const growth = num(prof.growth_per_turn);
            const output = num(prof.output_per_turn);
            if (turns !== null && calls !== null && growth !== null && output !== null) {
              usageProfiles[d][sz] = {
                turns,
                callsPerTurn: calls,
                growthPerTurn: Math.round(growth),
                outputPerTurn: Math.round(output),
              };
            }
          }
        }
      }
    }
  }

  const models = [...byId.values()];
  return {
    pack: {
      schemaVersion,
      packVersion: str(raw.pack_version, 64) ?? "unknown",
      generatedAt: str(raw.generated_at, 64),
      // Fall back to the domains models actually use when the pack lists none.
      domains: domains.length > 0 ? domains : [...new Set(models.flatMap((m) => Object.keys(m.skills)))],
      models,
      providers,
      successModel,
      usageProfiles,
    },
    warnings,
  };
}

function positive(v: unknown): number | null {
  const n = num(v);
  return n !== null && n > 0 ? Math.round(n) : null;
}
