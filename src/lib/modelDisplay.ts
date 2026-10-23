import type { ModelParams, ProviderId, ProviderInfo } from "./ipc";

const PROVIDER_NAMES: Record<ProviderId, string> = {
  CLAUDE_CODE: "Claude", CODEX_CLI: "Codex", CURSOR_CLI: "Cursor",
  ANTIGRAVITY_CLI: "Antigravity", COPILOT_CLI: "GitHub Copilot", AIDER: "Aider",
  OPENCODE: "OpenCode", GROK_CLI: "Grok", KIRO_CLI: "Kiro", QWEN_CODE: "Qwen",
  KIMI_CODE: "Kimi", QODER_CLI: "Qoder", AMP: "Amp", CRUSH: "Crush",
  GOOSE: "Goose", CLINE: "Cline", DROID: "Droid", AUGGIE: "Auggie",
  MUSE_CODE: "Muse", TRAE_CLI: "Trae", REASONIX: "Reasonix",
};

const WORDS: Record<string, string> = {
  gpt: "GPT", claude: "Claude", gemini: "Gemini", grok: "Grok", deepseek: "DeepSeek",
  qwen: "Qwen", kimi: "Kimi", glm: "GLM", llm: "LLM", api: "API", zdr: "ZDR",
};

const titleWord = (word: string) => WORDS[word.toLowerCase()] ?? word[0]?.toUpperCase() + word.slice(1);

export function providerDisplayName(provider: string, fallback?: string): string {
  return PROVIDER_NAMES[provider as ProviderId] ?? fallback ?? provider.split("_").map(titleWord).join(" ");
}

/** Friendly fallback for uncatalogued IDs; explicit display names keep their spelling. */
export function formatModelName(value: string): string {
  value = value.replace(/\s+(\d+[km])$/i, "[$1]");
  const contexts = [...value.matchAll(/\[\s*(\d+[km])\s*\]/gi)];
  let name = value.replace(/\s*\[\s*\d+[km]\s*\]/gi, "").trim();
  if (/^[a-z\d./_-]+$/.test(name)) {
    name = name.replace(/^([^/]+)\/\1(?=-)/, "$1")
      .replace(/^claude-(opus|sonnet|haiku|fable)-(\d+)-(\d+)(?=-|$)/, "claude-$1-$2.$3")
      .replace(/^claude-(\d+)-(\d+)-(opus|sonnet|haiku)/, "claude-$3-$1.$2");
    name = name.startsWith("gpt-")
      ? "GPT-" + name.slice(4).split("-").map(titleWord).join("-")
      : name.split(/[-_]/).filter(Boolean).map(titleWord).join(" ");
  }
  return name + contexts.map(match => `[${match[1].toUpperCase()}]`).join("");
}

export interface ModelDisplayInput {
  provider?: string | null;
  providerName?: string;
  requestedModel: string;
  modelName?: string;
  modelParams?: ModelParams | null;
  /** Already localized picker options; a context option is attached to the model name. */
  details?: string[];
}

/** Shared presentation only; never feed these labels back into routing or persistence. */
export function modelDisplay(model: ModelDisplayInput, providers: ProviderInfo[] = []) {
  const provider = providers.find(p => p.id === model.provider);
  const brand = model.provider ? providerDisplayName(model.provider, model.providerName ?? provider?.name) : "";
  const requested = model.requestedModel.toLowerCase();
  const group = provider?.modelGroups.find(g =>
    g.key.toLowerCase() === requested || g.name.toLowerCase() === requested || g.variants.some(v => v.id.toLowerCase() === requested));
  const variant = group?.variants.find(v => v.id.toLowerCase() === requested) ?? (group?.variants.length === 1 ? group.variants[0] : undefined);
  const catalogName = group?.name ?? provider?.models.find(m => m.id.toLowerCase() === requested)?.name;
  const contextDetail = model.details?.find(detail => /^\d+[km]$/i.test(detail));
  const context = contextDetail ?? model.requestedModel.match(/\[(\d+[km])\]$/i)?.[1] ?? variant?.context;
  let name = formatModelName(model.modelName || catalogName || model.requestedModel);
  if (brand === "Claude") name = name.replace(/^Claude[ -]/i, "");
  if (context && context !== "default") {
    name = name.replace(/\[\d+[km]\]/gi, "").replace(/\s+\d+[km]$/i, "");
    name += `[${context.toUpperCase()}]`;
  }
  const effort = model.modelParams?.effort ?? variant?.effort;
  const details = model.details?.filter(detail => detail !== contextDetail) ?? [
    effort && effort !== "default" ? titleWord(effort) : null,
    variant?.thinking ? "Thinking" : null,
    model.modelParams?.fast || variant?.fast ? "Fast" : null,
    model.modelParams?.tier ? titleWord(model.modelParams.tier) : null,
  ].filter((detail): detail is string => !!detail);
  return { provider: brand, name, details, label: [brand, name, ...details].filter(Boolean).join(" · ") };
}
