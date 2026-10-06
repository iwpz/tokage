import type { ModelGroup as CatalogModel, ProviderId, ProviderInfo } from "./ipc";

/**
 * Stand-in for the knowledge pack (docs/04 §3, knowledge-pack.md) until it ships: rough
 * per-model scores used to pre-select a model for each request mode.
 *   planning: decomposition / contract design (1–5)
 *   coding:   one-shot implementation quality (1–5)
 *   cost:     relative price tier (1 cheap – 5 expensive)
 * Rules match on the CLI model id; the first match wins.
 */
interface Capability {
  planning: number;
  coding: number;
  cost: number;
}

const RULES: [RegExp, Capability][] = [
  [/fable/i, { planning: 5.5, coding: 5, cost: 5 }],
  [/opus/i, { planning: 5, coding: 5, cost: 4 }],
  [/sonnet/i, { planning: 4, coding: 4.5, cost: 3 }],
  [/haiku/i, { planning: 2.5, coding: 3, cost: 1 }],
  [/gpt-6\.1-sol/i, { planning: 5, coding: 5, cost: 4.5 }],
  [/gpt-6-astra/i, { planning: 4.5, coding: 4.5, cost: 3.5 }],
  [/gpt-6-sol/i, { planning: 4.5, coding: 4.5, cost: 4 }],
  [/gpt-6-luna/i, { planning: 3.5, coding: 4, cost: 2 }],
  [/gpt-5\.6-sol/i, { planning: 4, coding: 4, cost: 3.5 }],
  [/gpt-5\.6-terra/i, { planning: 3.5, coding: 3.5, cost: 2.5 }],
  [/gpt-5\.6-luna/i, { planning: 3, coding: 3, cost: 1.5 }],
  [/pro/i, { planning: 4.5, coding: 4.5, cost: 3.5 }],
  [/flash/i, { planning: 3, coding: 3.5, cost: 1.5 }],
  [/deepseek/i, { planning: 3.5, coding: 4, cost: 1 }],
  [/mini|nano|lite/i, { planning: 2.5, coding: 3, cost: 1 }],
];

const FALLBACK: Capability = { planning: 3, coding: 3, cost: 3 };

// Internal routing / review aliases that should not be offered as chat models.
const HIDDEN = /^(codex-auto-review|gpt-reserve)$/;

export function capability(modelId: string): Capability {
  return RULES.find(([re]) => re.test(modelId))?.[1] ?? FALLBACK;
}

/**
 * First tree level label. Single-vendor CLIs show the vendor (OpenAI → GPT models);
 * multi-vendor CLIs (Aider, OpenCode, …) show their own name. Cursor shows its brand, with
 * the CLI name beside it.
 */
const VENDOR: Partial<Record<ProviderId, string>> = {
  CLAUDE_CODE: "Anthropic",
  CODEX_CLI: "OpenAI",
  ANTIGRAVITY_CLI: "Google",
  COPILOT_CLI: "GitHub",
  CURSOR_CLI: "Cursor",
  GROK_CLI: "xAI",
  KIRO_CLI: "Amazon",
  QWEN_CODE: "Alibaba",
  KIMI_CODE: "Moonshot",
};

export function vendorLabel(provider: ProviderId, fallback?: string): string {
  return VENDOR[provider] ?? fallback ?? provider;
}

/** Per-model settings chosen in the picker's submenu. Unset fields use the model's default. */
export interface ModelOptions {
  effort?: string;
  context?: string;
  thinking?: boolean;
  fast?: boolean;
  /** Auto routing profile (Copilot `--auto-tier`). */
  tier?: string;
}

/** `model` is the merged model key (ipc ModelGroup.key), not a concrete CLI id. */
export interface ModelRef {
  provider: ProviderId;
  model: string;
}

/** Key for per-model option storage. */
export const refKey = (r: ModelRef) => `${r.provider}:${r.model}`;

export type Pool = "router" | "own" | "third";

export interface ModelGroup {
  provider: ProviderInfo;
  models: CatalogModel[];
}

/**
 * Cursor bills its own models (Grok 4.5+, Composer; ids may carry a `cursor-` prefix) from
 * the Cursor Models pool and everything else from the third-party pool. Auto lets Cursor
 * pick the model and bills whichever pool that model belongs to, so it sits apart.
 * Other CLIs have no pools.
 */
export function poolOf(provider: ProviderId, key: string): Pool | null {
  if (provider !== "CURSOR_CLI") return null;
  if (/^auto$/i.test(key)) return "router";
  return /^(cursor-|grok|composer)/i.test(key) ? "own" : "third";
}

/** High-tier and medium-tier costly keywords for tiered banning */
export const TIER_MODERATE_KEYWORDS = [
  "fable",
  "astra",
  "gpt-6-astra",
];

export const TIER_STRICT_KEYWORDS = [
  ...TIER_MODERATE_KEYWORDS,
  "opus",
  "claude-opus",
  "sol",
  "gpt-6.1-sol",
  "gpt-6-sol",
  "gpt-5.6-sol",
];

/** Checks if a model key, name, variant, or provider-qualified name matches the blocked list or banning level. */
export function isModelBlocked(
  keyOrName: string,
  blockedModels: string[] = [],
  providerIdOrName?: string,
  banningLevel: "off" | "moderate" | "strict" = "off",
  modelProviderPreferences?: Record<string, { prefer?: string; never?: string[] }>,
): boolean {
  const target = keyOrName.trim().toLowerCase();
  if (!target) return false;
  const prov = providerIdOrName ? providerIdOrName.trim().toLowerCase() : "";

  // 1. Check Banning Level
  if (banningLevel === "strict") {
    if (TIER_STRICT_KEYWORDS.some((kw) => target.includes(kw))) return true;
  } else if (banningLevel === "moderate") {
    if (TIER_MODERATE_KEYWORDS.some((kw) => target.includes(kw))) return true;
  }

  // 2. Check Provider Soft-Banning ("never" list)
  if (prov && modelProviderPreferences) {
    for (const [modelKey, pref] of Object.entries(modelProviderPreferences)) {
      if (pref.never && pref.never.length > 0 && target.includes(modelKey.toLowerCase())) {
        const isNever = pref.never.some(
          (n) =>
            prov === n.toLowerCase() ||
            prov.includes(n.toLowerCase()) ||
            n.toLowerCase().includes(prov),
        );
        if (isNever) return true;
      }
    }
  }

  // 3. Check Explicit Blocked Models List
  if (!blockedModels || blockedModels.length === 0) return false;

  return blockedModels.some((b) => {
    const rawPattern = b.trim();
    if (!rawPattern) return false;
    const pattern = rawPattern.toLowerCase();

    // Direct equality with target
    if (target === pattern) return true;

    // Check if blocked entry is in "Provider · Model" format (e.g. "Codex CLI · GPT 6 Astra" or "CODEX_CLI:gpt-6")
    if (rawPattern.includes("·") || rawPattern.includes(":")) {
      const parts = rawPattern.includes("·")
        ? rawPattern.split("·").map((p) => p.trim().toLowerCase())
        : rawPattern.split(":").map((p) => p.trim().toLowerCase());
      if (parts.length >= 2) {
        const blockedProv = parts[0];
        const blockedModel = parts.slice(1).join("·").trim();
        // If provider matches (or not given) and model matches
        const provMatches = !prov || prov === blockedProv || prov.includes(blockedProv) || blockedProv.includes(prov);
        const modelMatches = target === blockedModel || target.includes(blockedModel) || blockedModel.includes(target);
        if (provMatches && modelMatches) return true;
      }
    }

    // Fallback: fuzzy/substring match
    return target === pattern || target.includes(pattern) || pattern.includes(target);
  });
}

/**
 * Providers that can take a turn right now, with their selectable models. A CLI the user
 * hasn't allowed to receive workspace trust is excluded, from manual picks and auto alike.
 * Any model matching `blockedModels` is completely excluded from selection and recommendation.
 */
export function selectableGroups(
  providers: ProviderInfo[],
  enabled: Partial<Record<ProviderId, boolean>>,
  trust: Partial<Record<ProviderId, boolean>>,
  mode?: PickMode,
  blockedModels: string[] = [],
  banningLevel: "off" | "moderate" | "strict" = "off",
  modelProviderPreferences?: Record<string, { prefer?: string; never?: string[] }>,
): ModelGroup[] {
  return providers
    // Only CLIs Tokage has a headless adapter for can take turns.
    .filter((p) => p.chat === true && runnable(p) && enabled[p.id] !== false && trust[p.id] === true)
    // A CLI whose config would run commands unasked never takes Direct turns (docs/11 §4.2).
    .filter((p) => mode !== "direct" || !directBlocked(p))
    .map((p) => ({
      provider: p,
      // Aider's `prefix/` placeholder means "key found, list unavailable" and is not a model.
      models: p.modelGroups.filter(
        (m) =>
          !HIDDEN.test(m.key) &&
          !m.key.endsWith("/") &&
          !isModelBlocked(m.key, blockedModels, p.id, banningLevel, modelProviderPreferences) &&
          !isModelBlocked(m.key, blockedModels, p.name, banningLevel, modelProviderPreferences) &&
          !isModelBlocked(m.name, blockedModels, p.id, banningLevel, modelProviderPreferences) &&
          !isModelBlocked(m.name, blockedModels, p.name, banningLevel, modelProviderPreferences) &&
          !m.variants.some(
            (v) =>
              isModelBlocked(v.id, blockedModels, p.id, banningLevel, modelProviderPreferences) ||
              isModelBlocked(v.id, blockedModels, p.name, banningLevel, modelProviderPreferences),
          ),
      ),
    }))
    .filter((g) => g.models.length > 0);
}

/** Signed in and able to take turns; an untested version still runs (docs/01 §4). */
export function runnable(p: ProviderInfo) {
  return p.status === "READY" || p.status === "UNSUPPORTED_VERSION";
}

/** Direct mode is refused on this CLI (see `DirectLimit`). */
export function directBlocked(p: ProviderInfo) {
  return p.directLimit?.code === "CURSOR_UNSAFE_PERMISSIONS";
}

/** What the submenu can offer for one model; a dimension is shown only when it has a choice. */
export function optionAxes(m: CatalogModel) {
  const thinkingValues = new Set(m.variants.map((v) => v.thinking));
  // Single-id models take speed as a parameter; multi-id ones need both tiers listed.
  const fastValues = new Set(m.variants.map((v) => v.fast));
  return {
    efforts: m.efforts,
    contexts: m.contexts,
    thinking: thinkingValues.size > 1,
    fast: m.variants.length === 1 ? m.fast : fastValues.size > 1,
    tiers: m.tiers,
  };
}

export function hasOptions(m: CatalogModel) {
  const a = optionAxes(m);
  return a.efforts.length > 1 || a.contexts.length > 1 || a.thinking || a.fast || a.tiers.length > 1;
}

/** Explicit options filled in with the model's defaults. */
export function effectiveOptions(m: CatalogModel, o: ModelOptions = {}): Required<ModelOptions> {
  const a = optionAxes(m);
  const pick = (list: string[], want: string | undefined, fallback: string | null) =>
    want && list.includes(want) ? want : fallback && list.includes(fallback) ? fallback : (list.find((x) => x === "medium") ?? list.find((x) => x === "default") ?? list[0] ?? "");
  return {
    effort: pick(a.efforts, o.effort, m.defaultEffort),
    context: pick(a.contexts, o.context, "default"),
    thinking: a.thinking ? (o.thinking ?? false) : m.variants.some((v) => v.thinking),
    fast: a.fast ? (o.fast ?? false) : false,
    tier: pick(a.tiers, o.tier, m.defaultTier),
  };
}

/** Concrete CLI id plus any parameters for the chosen options. */
export function resolveVariant(
  m: CatalogModel,
  o: ModelOptions = {},
): { id: string; effort: string | null; fast: boolean; tier: string | null } {
  const e = effectiveOptions(m, o);
  if (m.variants.length === 1) {
    const effort = m.efforts.length > 0 && e.effort !== "default" ? e.effort : null;
    return { id: m.variants[0].id, effort, fast: e.fast, tier: e.tier || null };
  }
  // Not every combination exists (Cursor lists Fast for some efforts only): closest wins,
  // weighted so effort and context matter more than the speed tier.
  const m_ = matcher(m, e);
  const score = (v: CatalogModel["variants"][number]) =>
    (m_.effort(v) ? 8 : 0) + (m_.context(v) ? 4 : 0) + (m_.thinking(v) ? 2 : 0) + (m_.fast(v) ? 1 : 0);
  const best = [...m.variants].sort((a, b) => score(b) - score(a))[0];
  return { id: best.id, effort: null, fast: false, tier: null };
}

/** Whether a variant exists for these options with the given axis flipped on. */
export function available(m: CatalogModel, o: ModelOptions, patch: ModelOptions): boolean {
  if (m.variants.length === 1) return true;
  const t = matcher(m, effectiveOptions(m, { ...o, ...patch }));
  return m.variants.some((v) => t.effort(v) && t.context(v) && t.thinking(v) && t.fast(v));
}

/** Per-axis variant tests; an axis without a choice matches every variant. */
function matcher(m: CatalogModel, e: Required<ModelOptions>) {
  const a = optionAxes(m);
  type V = CatalogModel["variants"][number];
  return {
    effort: (v: V) => a.efforts.length < 2 || (v.effort ?? "default") === e.effort,
    context: (v: V) => a.contexts.length < 2 || (v.context ?? "default") === e.context,
    thinking: (v: V) => !a.thinking || v.thinking === e.thinking,
    fast: (v: V) => !a.fast || v.fast === e.fast,
  };
}

export function findModel(groups: ModelGroup[], ref: ModelRef | null): CatalogModel | null {
  if (!ref) return null;
  return groups.find((g) => g.provider.id === ref.provider)?.models.find((m) => m.key === ref.model) ?? null;
}

export type PickMode = "plan" | "direct";

/** Minimum coding score for direct execution (stand-in for docs/04 §5 p_min). */
const DIRECT_BAR = 4;

/**
 * Plan: the strongest planner wins; price only breaks ties (docs/04 §5, planner role).
 * Direct: among models that clear the coding bar, the cheapest wins (docs/04 §2);
 * if none clears it, the best coder.
 */
export function recommend(
  groups: ModelGroup[],
  mode: PickMode,
  modelProviderPreferences?: Record<string, { prefer?: string; never?: string[] }>,
): ModelRef | null {
  const all = groups.flatMap((g) =>
    g.models.map((m) => {
      let prefBonus = 0;
      if (modelProviderPreferences) {
        for (const [modelKey, pref] of Object.entries(modelProviderPreferences)) {
          if (m.key.toLowerCase().includes(modelKey.toLowerCase())) {
            if (pref.prefer && g.provider.id.toLowerCase() === pref.prefer.toLowerCase()) {
              prefBonus = 10; // Prioritize this provider when selecting this model
            }
          }
        }
      }
      return {
        ref: { provider: g.provider.id, model: m.key },
        c: capability(m.key),
        prefBonus,
      };
    }),
  );
  if (all.length === 0) return null;
  const order =
    mode === "plan"
      ? (a: { c: Capability; prefBonus: number }, b: { c: Capability; prefBonus: number }) =>
          b.prefBonus - a.prefBonus || b.c.planning - a.c.planning || a.c.cost - b.c.cost
      : (a: { c: Capability; prefBonus: number }, b: { c: Capability; prefBonus: number }) => {
          if (a.prefBonus !== b.prefBonus) return b.prefBonus - a.prefBonus;
          const qa = a.c.coding >= DIRECT_BAR,
            qb = b.c.coding >= DIRECT_BAR;
          if (qa !== qb) return qa ? -1 : 1;
          return qa ? a.c.cost - b.c.cost || b.c.coding - a.c.coding : b.c.coding - a.c.coding || a.c.cost - b.c.cost;
        };
  return [...all].sort((x, y) => order(x, y))[0].ref;
}

/** Short summary of non-default options, e.g. ["High", "1M", "Fast"]; labels via `label`. */
export function optionSummary(m: CatalogModel, o: ModelOptions | undefined, label: (axis: "effort" | "context" | "tier", v: string) => string, words: { thinking: string; fast: string }): string[] {
  const a = optionAxes(m);
  const e = effectiveOptions(m, o);
  return [
    a.tiers.length > 1 ? label("tier", e.tier) : null,
    a.efforts.length > 1 && e.effort !== "default" ? label("effort", e.effort) : null,
    a.contexts.length > 1 && e.context !== "default" ? label("context", e.context) : null,
    a.thinking && e.thinking ? words.thinking : null,
    a.fast && e.fast ? words.fast : null,
  ].filter((x): x is string => !!x);
}

export function sameRef(a: ModelRef | null, b: ModelRef | null) {
  return !!a && !!b && a.provider === b.provider && a.model === b.model;
}

/**
 * Local, zero-token triage for auto mode (docs/02 §2, P5.m4): multi-domain or feature-level
 * requests go through planning; single issues / pointed questions go direct. Ambiguous
 * requests default to lightweight plan.
 */
export function triage(text: string): PickMode {
  const t = text.trim().toLowerCase();
  if (!t) return "plan";

  const planWords =
    /实现|开发|搭建|重构|设计一个|新增功能|整个项目|模块|架构|全套|流程|从零|迁移到|implement|build|scaffold|create an? (?:app|service|module|feature|system)|migrate (?:to|from)|refactor (?:the )?(?:whole|entire)|from scratch|end[- ]to[- ]end|architecture/i;
  const directWords =
    /这个报错|报错|解释|为什么|是什么意思|改一下|修复|修一下|这个函数|这一行|单测报错|编译报错|语法错误|拼写|this error|explain|what does|why does|fix (?:this|the) (?:typo|bug|line|function|test)|rename|stack ?trace|quick fix|what is|how to use|syntax error/i;
  const pinpointPattern = /[\w./\\-]+\.[a-zA-Z]{1,6}:\d+|(?:line|at line|第)\s*\d+\s*(?:行)?/i;

  const domains = [
    /组件|样式|页面|界面|交互|前端|css|tailwind|react|vue|svelte|component|ui|layout|a11y|accessibility|html/,
    /接口|api|服务端|后端|鉴权|endpoint|handler|route|auth|middleware|grpc|rest|controller|rpc|jwt/,
    /数据库|表结构|迁移|查询|sql|schema|migration|index|orm|sqlite|postgres|mysql|database|redis/,
    /算法|性能|复杂度|优化|algorithm|performance|complexity|optimize|concurrency|cache/,
    /联调|合并冲突|集成|integration|merge conflict|wire up|end-to-end/,
    /重构|迁移|重命名|拆分|解耦|refactor|rename|extract|restructure|upgrade/,
    /构建|部署|ci|docker|k8s|pipeline|build|deploy|github actions|terraform|nginx/,
    /测试|单测|用例|覆盖率|test|spec|coverage|e2e|mock/,
    /文档|注释|readme|docs|documentation|changelog|comment/,
    /评审|审查|review|audit|检查一下/,
  ].filter((re) => re.test(t)).length;

  const fileCount = (t.match(/(?:[\w-]+\/)+[\w-]+\.[a-zA-Z0-9]{1,8}|[\w-]+\.(?:rs|ts|tsx|js|jsx|py|go|java|c|cpp|h|hpp|vue|svelte|css|scss|html|json|yaml|yml|toml|sql|md|sh)/g) ?? []).length;

  if (domains >= 2 || planWords.test(t) || fileCount >= 4) return "plan";
  if (directWords.test(t) || pinpointPattern.test(t)) return "direct";

  // When lacking substantive direct signals, default to PLAN as specified in docs/02 §2.
  return "plan";
}
