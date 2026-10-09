import type { ProviderId, ProviderInfo } from "./ipc";

/** Per-family channel allow matrix entry. Missing/true = allowed; false = denied. */
export type ModelChannelEntry = {
  prefer?: string;
  channels: Partial<Record<string, boolean>>;
  /** Deny this family on every channel (tier ban / global block). */
  denyAll?: boolean;
  /** User-added row from the probed catalog (not a builtin family). */
  custom?: boolean;
  /** Display name for custom rows. */
  label?: string;
  /** Substring used to match model ids/names for custom rows (defaults to the entry key). */
  needle?: string;
};

export type ModelChannelAccess = Record<string, ModelChannelEntry>;

export type ModelFamilyDef = {
  key: string;
  name: string;
  /** Match model id / display name (case-insensitive). First match wins. */
  match: RegExp;
  /** Included in one-click “ban ultra-expensive”. */
  ultra?: boolean;
  /** Builtin rows are never removed from the matrix. */
  builtin?: boolean;
};

/** A row shown in the matrix UI (builtin or user-added). */
export type MatrixFamilyRow = {
  key: string;
  name: string;
  ultra?: boolean;
  custom?: boolean;
};

/**
 * Ordered families for the allow matrix. Ultra families are listed first so
 * `familyOf("gpt-6-astra")` resolves to `astra`, not a broader gpt-6 bucket.
 */
export const MODEL_FAMILIES: ModelFamilyDef[] = [
  { key: "fable", name: "Claude Fable", match: /fable/i, ultra: true, builtin: true },
  { key: "astra", name: "GPT-6 Astra", match: /astra/i, ultra: true, builtin: true },
  { key: "opus", name: "Claude Opus", match: /opus/i, ultra: true, builtin: true },
  { key: "sol", name: "GPT Sol", match: /(?:^|[-_/.\s])sol(?:$|[-_/.\s])/i, ultra: true, builtin: true },
  { key: "sonnet", name: "Claude Sonnet", match: /sonnet/i, builtin: true },
  { key: "deepseek", name: "DeepSeek V4 Pro / Coder", match: /deepseek/i, builtin: true },
  { key: "gpt-6", name: "GPT-6 / Luna", match: /gpt-6/i, builtin: true },
];

export const ULTRA_FAMILY_KEYS = MODEL_FAMILIES.filter((f) => f.ultra).map((f) => f.key);

const BUILTIN_KEYS = new Set(MODEL_FAMILIES.map((f) => f.key));

/** Resolve which matrix row a model id/name belongs to (builtins first, then custom access rows). */
export function familyOf(modelIdOrName: string, access?: ModelChannelAccess): string | null {
  const t = modelIdOrName.trim();
  if (!t) return null;
  for (const f of MODEL_FAMILIES) {
    if (f.match.test(t)) return f.key;
  }
  if (access) {
    const lower = t.toLowerCase();
    for (const [key, entry] of Object.entries(access)) {
      if (!entry?.custom || BUILTIN_KEYS.has(key)) continue;
      const needle = (entry.needle || key).trim().toLowerCase();
      if (!needle) continue;
      if (lower === needle || lower.includes(needle) || needle.includes(lower)) return key;
    }
  }
  return null;
}

/** Builtin rows + user-added custom rows from access. */
export function matrixFamilyRows(access?: ModelChannelAccess): MatrixFamilyRow[] {
  const rows: MatrixFamilyRow[] = MODEL_FAMILIES.map((f) => ({
    key: f.key,
    name: f.name,
    ultra: f.ultra,
    custom: false,
  }));
  if (!access) return rows;
  for (const [key, entry] of Object.entries(access)) {
    if (!entry?.custom || BUILTIN_KEYS.has(key)) continue;
    if (rows.some((r) => r.key === key)) continue;
    rows.push({
      key,
      name: entry.label || key,
      custom: true,
    });
  }
  return rows;
}

/** Stable key for a probed catalog model when adding a custom matrix row. */
export function catalogFamilyKey(modelKey: string, modelName: string): string {
  const builtin = familyOf(modelKey) ?? familyOf(modelName);
  if (builtin) return builtin;
  const raw = (modelKey || modelName).trim().toLowerCase();
  return raw.replace(/[^a-z0-9._/-]+/g, "-").replace(/^-+|-+$/g, "") || raw;
}

/** Only channels that actually expose a matching model. */
export function providerOffersModel(
  p: {
    models: { id: string; name: string }[];
    modelGroups: { key: string; name: string }[];
  },
  modelKey: string,
): boolean {
  const key = modelKey.trim().toLowerCase();
  if (!key) return false;
  if (p.modelGroups.some((g) => g.key.toLowerCase().includes(key) || g.name.toLowerCase().includes(key))) {
    return true;
  }
  return p.models.some((m) => m.id.toLowerCase().includes(key) || m.name.toLowerCase().includes(key));
}

function normalizeProviderToken(token: string): string {
  return token.trim().toLowerCase().replace(/[\s-]+/g, "_");
}

/** Loose match: ProviderId, display name, or alias. */
export function providerTokenMatches(providerIdOrName: string, token: string): boolean {
  const prov = normalizeProviderToken(providerIdOrName);
  const t = normalizeProviderToken(token);
  if (!prov || !t) return false;
  return prov === t || prov.includes(t) || t.includes(prov);
}

/**
 * Whether (provider, model) is allowed by the channel matrix.
 * When access is empty/undefined for the family, default is allow.
 */
export function isChannelDenied(
  modelIdOrName: string,
  providerIdOrName: string | undefined,
  access: ModelChannelAccess | undefined,
): boolean {
  if (!access) return false;
  const family = familyOf(modelIdOrName, access);
  if (!family) return false;
  const entry = access[family];
  if (!entry) return false;
  if (entry.denyAll) return true;
  if (!providerIdOrName) return false;
  for (const [id, allowed] of Object.entries(entry.channels || {})) {
    if (allowed === false && providerTokenMatches(providerIdOrName, id)) return true;
  }
  return false;
}

/** Prefer map for recommend(): family key → preferred ProviderId. */
export function preferMapFromAccess(access: ModelChannelAccess | undefined): Record<string, { prefer?: string }> {
  if (!access) return {};
  const out: Record<string, { prefer?: string }> = {};
  for (const [key, entry] of Object.entries(access)) {
    if (entry.prefer) out[key] = { prefer: entry.prefer };
  }
  return out;
}

export function ensureFamilyEntry(access: ModelChannelAccess, family: string): ModelChannelEntry {
  if (!access[family]) access[family] = { channels: {} };
  if (!access[family].channels) access[family].channels = {};
  return access[family];
}

/** Default shipping policy (mirrors former opus prefer/never defaults). */
export function defaultModelChannelAccess(): ModelChannelAccess {
  return {
    opus: {
      prefer: "CLAUDE_CODE",
      channels: { CURSOR_CLI: false, COPILOT_CLI: false },
    },
  };
}

/**
 * Build matrix from legacy blockedModels / banningLevel / modelProviderPreferences.
 */
export function migrateToModelChannelAccess(input: {
  blockedModels?: string[];
  modelBanningLevel?: "off" | "moderate" | "strict";
  modelProviderPreferences?: Record<string, { prefer?: string; never?: string[] }>;
  modelChannelAccess?: ModelChannelAccess;
}): ModelChannelAccess {
  // Already migrated with content — keep, but still merge legacy if access empty
  if (input.modelChannelAccess && Object.keys(input.modelChannelAccess).length > 0) {
    return structuredClone(input.modelChannelAccess);
  }

  const access = defaultModelChannelAccess();

  // Preferences → prefer + channel denies
  for (const [key, pref] of Object.entries(input.modelProviderPreferences || {})) {
    const family = familyOf(key) ?? key.trim().toLowerCase();
    if (!family) continue;
    const entry = ensureFamilyEntry(access, family);
    if (pref.prefer) entry.prefer = pref.prefer;
    for (const neverId of pref.never || []) {
      entry.channels[neverId] = false;
    }
  }

  // Tier ban → denyAll on those families
  const level = input.modelBanningLevel ?? "off";
  const tierFamilies =
    level === "strict" ? ULTRA_FAMILY_KEYS : level === "moderate" ? (["fable", "astra"] as string[]) : [];
  for (const f of tierFamilies) {
    ensureFamilyEntry(access, f).denyAll = true;
  }

  // Explicit blocked list
  for (const raw of input.blockedModels || []) {
    const pattern = raw.trim();
    if (!pattern) continue;

    if (pattern.includes("·") || pattern.includes(":")) {
      const parts = pattern.includes("·")
        ? pattern.split("·").map((p) => p.trim())
        : pattern.split(":").map((p) => p.trim());
      if (parts.length >= 2) {
        const prov = parts[0];
        const modelPart = parts.slice(1).join("·").trim();
        const family = familyOf(modelPart) ?? familyOf(pattern);
        if (family) {
          const entry = ensureFamilyEntry(access, family);
          // Try to store as ProviderId-ish token
          const idGuess = prov.toUpperCase().replace(/[\s-]+/g, "_");
          entry.channels[idGuess] = false;
          // Also keep original token for name matching
          if (idGuess !== prov) entry.channels[prov] = false;
        } else {
          // Unknown family: invent a key from the model token
          const custom = modelPart.toLowerCase().replace(/\s+/g, "-");
          ensureFamilyEntry(access, custom).channels[prov.toUpperCase().replace(/[\s-]+/g, "_")] = false;
        }
        continue;
      }
    }

    const family = familyOf(pattern);
    if (family) {
      ensureFamilyEntry(access, family).denyAll = true;
    } else {
      const custom = pattern.toLowerCase().replace(/\s+/g, "-");
      ensureFamilyEntry(access, custom).denyAll = true;
    }
  }

  return access;
}

/** Ban all ultra families everywhere (one-click). */
export function banUltraFamilies(access: ModelChannelAccess): ModelChannelAccess {
  const next = structuredClone(access);
  for (const key of ULTRA_FAMILY_KEYS) {
    ensureFamilyEntry(next, key).denyAll = true;
  }
  return next;
}

/**
 * Toggle whether a family is allowed on one provider.
 * When lifting `denyAll` by allowing one channel, other currently-known denials stay;
 * callers that pass `otherProviderIds` will keep those denied so only the clicked
 * channel turns on.
 */
export function setFamilyChannel(
  access: ModelChannelAccess,
  family: string,
  providerId: ProviderId | string,
  allowed: boolean,
  otherProviderIds: string[] = [],
): ModelChannelAccess {
  const next = structuredClone(access);
  const entry = ensureFamilyEntry(next, family);
  if (allowed) {
    if (entry.denyAll) {
      entry.denyAll = false;
      for (const id of otherProviderIds) {
        if (id !== providerId) entry.channels[id] = false;
      }
    }
    delete entry.channels[providerId];
  } else {
    entry.channels[providerId] = false;
  }
  return next;
}

/** Whether the family is currently allowed on this provider (UI checkbox state). */
export function isFamilyAllowedOnChannel(
  access: ModelChannelAccess | undefined,
  family: string,
  providerId: string,
): boolean {
  const entry = access?.[family];
  if (!entry) return true;
  if (entry.denyAll) return false;
  return entry.channels[providerId] !== false;
}

/** Providers from the live catalog that offer this family. */
export function channelsForFamily(
  providers: ProviderInfo[],
  familyKey: string,
  access?: ModelChannelAccess,
): ProviderInfo[] {
  const needle = access?.[familyKey]?.needle || familyKey;
  return providers.filter((p) => providerOffersModel(p, needle) || providerOffersModel(p, familyKey));
}

/** Deduped probed models across runnable providers (for the add-row dropdown). */
export function probedCatalogModels(providers: ProviderInfo[]): { key: string; name: string }[] {
  const seen = new Set<string>();
  const list: { key: string; name: string }[] = [];
  for (const p of providers) {
    if (p.modelGroups?.length) {
      for (const g of p.modelGroups) {
        const key = (g.key || g.name || "").trim();
        if (!key) continue;
        const norm = key.toLowerCase();
        if (seen.has(norm)) continue;
        seen.add(norm);
        list.push({ key, name: g.name || g.key });
      }
    } else {
      for (const m of p.models || []) {
        const key = (m.id || m.name || "").trim();
        if (!key) continue;
        const norm = key.toLowerCase();
        if (seen.has(norm)) continue;
        seen.add(norm);
        list.push({ key, name: m.name || m.id });
      }
    }
  }
  return list.sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
}

/** Add a custom matrix row from a probed catalog model. Returns null if already covered by a builtin/custom row. */
export function addCustomFamilyFromCatalog(
  access: ModelChannelAccess,
  modelKey: string,
  modelName: string,
): { access: ModelChannelAccess; key: string; alreadyPresent: boolean } | null {
  const builtin = familyOf(modelKey) ?? familyOf(modelName);
  if (builtin) {
    return { access, key: builtin, alreadyPresent: true };
  }
  const key = catalogFamilyKey(modelKey, modelName);
  if (access[key]?.custom) {
    return { access, key, alreadyPresent: true };
  }
  // Already matched as another custom row?
  const existing = familyOf(modelKey, access) ?? familyOf(modelName, access);
  if (existing) {
    return { access, key: existing, alreadyPresent: true };
  }
  const next = structuredClone(access);
  next[key] = {
    channels: {},
    custom: true,
    label: modelName || modelKey,
    needle: modelKey || modelName,
  };
  return { access: next, key, alreadyPresent: false };
}

export function removeCustomFamily(access: ModelChannelAccess, familyKey: string): ModelChannelAccess {
  if (BUILTIN_KEYS.has(familyKey)) return access;
  const next = structuredClone(access);
  delete next[familyKey];
  return next;
}
