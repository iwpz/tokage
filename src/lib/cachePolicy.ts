import i18n from "../i18n";
export type CacheType = "ephemeral" | "disk_persistent" | "implicit" | "none";

export interface ModelCachePolicy {
  cacheType: CacheType;
  /** Cache TTL in seconds (for ephemeral or persistent caches). 0 if implicit or none. */
  ttlSeconds: number;
  /** Human-readable label for tooltips and info. */
  label: string;
}

export interface SessionCacheStatus {
  cacheType: CacheType;
  isWarm: boolean;
  ttlRemainingSeconds: number;
  elapsedSeconds: number;
  ttlSeconds: number;
  label: string;
}

/** Canonical spelling shared with tokage-context::inputs. */
export function canonicalCacheType(mode: string): CacheType {
  if (["standard","ephemeral","explicit_prefix","automatic_prefix"].includes(mode)) return "ephemeral";
  if (mode==="disk_persistent" || mode==="implicit") return mode;
  return "none";
}
export function contextCacheWarmth(elapsed: number, ttl: number, mode: string, reliability = 0): {isWarm:boolean;ttlRemainingSeconds:number} {
  const kind=canonicalCacheType(mode);
  if(kind==="none" || !Number.isFinite(ttl) || ttl<=0 || !Number.isFinite(elapsed) || elapsed<0) return {isWarm:false,ttlRemainingSeconds:0};
  const effective=ttl*0.9;
  return {isWarm:elapsed<effective && (kind!=="implicit" || reliability>=0.8),ttlRemainingSeconds:Math.max(0,effective-elapsed)};
}

/**
 * Resolves the cache architecture and TTL for a given model or provider.
 * Follows provider specifications and docs/05 §5:
 * - Claude / Anthropic: explicit prefix caching with 5-minute (300s) TTL
 * - Codex / OpenAI: automatic prefix caching with 30-minute (1800s) TTL
 * - DeepSeek: disk-persistent prompt cache with long retention (e.g. 24h / 86400s)
 * - Kimi / Moonshot: automatic prefix caching with 10-minute (600s) TTL
 * - Grok / xAI: automatic prefix caching with 5-minute (300s) TTL
 * - Gemini / Qwen: implicit server-managed opportunistic cache
 * - Unknown / unsupported: no cache ("none")
 */
export function resolveModelCachePolicy(
  modelName?: string,
  provider?: string,
): ModelCachePolicy {
  const m = (modelName ?? "").toLowerCase();
  const p = (provider ?? "").toUpperCase();

  // DeepSeek: Disk-persistent prompt cache (docs/05 §5)
  if (m.includes("deepseek") || p.includes("DEEPSEEK")) {
    return {
      cacheType: "disk_persistent",
      ttlSeconds: 86400,
      label: i18n.t("cachePolicy.persistent"),
    };
  }

  // Claude / Anthropic: Explicit prefix cache, 5-minute TTL
  if (
    p === "CLAUDE_CODE" ||
    m.includes("claude") ||
    m.includes("sonnet") ||
    m.includes("opus") ||
    m.includes("haiku") ||
    m.includes("fable")
  ) {
    return {
      cacheType: "ephemeral",
      ttlSeconds: 300,
      label: i18n.t("cachePolicy.explicitPrefix"),
    };
  }

  // OpenAI / Codex: Automatic prefix cache, 30-minute TTL
  if (
    p === "CODEX_CLI" ||
    m.includes("gpt-") ||
    m.includes("codex") ||
    m.includes("o1") ||
    m.includes("o3") ||
    m.includes("o4")
  ) {
    return {
      cacheType: "ephemeral",
      ttlSeconds: 1800,
      label: i18n.t("cachePolicy.autoPrefix"),
    };
  }

  // Kimi / Moonshot: Automatic prefix cache, 10-minute TTL
  if (p === "KIMI_CODE" || m.includes("kimi") || m.includes("moonshot")) {
    return {
      cacheType: "ephemeral",
      ttlSeconds: 600,
      label: i18n.t("cachePolicy.autoPrefix"),
    };
  }

  // Grok / xAI: Automatic prefix cache, 5-minute TTL
  if (p === "GROK_CLI" || m.includes("grok")) {
    return {
      cacheType: "ephemeral",
      ttlSeconds: 300,
      label: i18n.t("cachePolicy.autoPrefix"),
    };
  }

  // Implicit / Opportunistic caching: Gemini, Qwen, Antigravity CLI
  if (
    p === "ANTIGRAVITY_CLI" ||
    p === "QWEN_CODE" ||
    m.includes("gemini") ||
    m.includes("qwen")
  ) {
    return {
      cacheType: "implicit",
      ttlSeconds: 0,
      label: i18n.t("cachePolicy.serverAuto"),
    };
  }

  return {
    cacheType: "none",
    ttlSeconds: 0,
    label: i18n.t("cachePolicy.none"),
  };
}

/**
 * Calculates real-time session cache warmth and remaining TTL based on the last
 * turn completion timestamp and specific model caching policy.
 */
export function calculateSessionCacheStatus(
  lastAssistantEndedAt?: number,
  modelName?: string,
  provider?: string,
  now = Date.now(),
): SessionCacheStatus {
  const policy = resolveModelCachePolicy(modelName, provider);

  if (policy.cacheType === "none" || !lastAssistantEndedAt) {
    return {
      cacheType: policy.cacheType,
      isWarm: false,
      ttlRemainingSeconds: 0,
      elapsedSeconds: 0,
      ttlSeconds: policy.ttlSeconds,
      label: policy.label,
    };
  }

  const elapsedSeconds = Math.max(0, Math.floor((now - lastAssistantEndedAt) / 1000));

  const warmth = contextCacheWarmth(elapsedSeconds,policy.ttlSeconds,policy.cacheType);
  return {cacheType:policy.cacheType,...warmth,elapsedSeconds,ttlSeconds:policy.ttlSeconds,label:policy.label};
}
