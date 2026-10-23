import type { ProviderId } from "./ipc";
import type { RuntimeModel } from "./runtime";
import { useProviders } from "../stores/providers";

const PROVIDER_NAMES: Record<ProviderId, string> = {
  CLAUDE_CODE: "Claude", CODEX_CLI: "Codex", CURSOR_CLI: "Cursor",
  ANTIGRAVITY_CLI: "Antigravity", COPILOT_CLI: "GitHub Copilot", AIDER: "Aider",
  OPENCODE: "OpenCode", GROK_CLI: "Grok", KIRO_CLI: "Kiro", QWEN_CODE: "Qwen",
  KIMI_CODE: "Kimi", QODER_CLI: "Qoder", AMP: "Amp", CRUSH: "Crush",
  GOOSE: "Goose", CLINE: "Cline", DROID: "Droid", AUGGIE: "Auggie",
  MUSE_CODE: "Muse", TRAE_CLI: "Trae", REASONIX: "Reasonix",
};

function friendlyModelName(name: string): string {
  return name
    .replace(/^(?:claude[- ])?(opus|sonnet|haiku|fable)(?=$|[-\s\[])/i, (_, family: string) => family[0].toUpperCase() + family.slice(1).toLowerCase())
    .replace(/^(auto|default)$/i, value => value[0].toUpperCase() + value.slice(1).toLowerCase())
    .replace(/\s*\[(\d+[km])\]/gi, (_, size: string) => ` [${size.toUpperCase()}]`)
    .trim();
}

/** Presentation only: runtime assignments and select values retain their original keys. */
export function useModelLabel() {
  const providers = useProviders(state => state.providers);
  return (model: Pick<RuntimeModel, "provider" | "requestedModel" | "modelParams">): string => {
    const provider = providers.find(p => p.id === model.provider);
    const brand = PROVIDER_NAMES[model.provider as ProviderId] ?? provider?.name ?? model.provider;
    const requested = model.requestedModel.toLowerCase();
    const group = provider?.modelGroups.find(g =>
      g.key.toLowerCase() === requested || g.variants.some(v => v.id.toLowerCase() === requested));
    const variant = group?.variants.find(v => v.id.toLowerCase() === requested);
    const catalogName = group?.name ?? provider?.models.find(m => m.id.toLowerCase() === requested)?.name;
    // A concrete context suffix must remain visible even if the catalog groups it away.
    const context = model.requestedModel.match(/\[(\d+[km])\]$/i)?.[1] ?? variant?.context;
    let name = friendlyModelName(catalogName || model.requestedModel);
    if (context && context !== "default") {
      name = name.replace(/\s*\[\d+[km]\]/gi, "").replace(/\s+\d+[km]$/i, "");
      name += ` [${context.toUpperCase()}]`;
    }
    const effort = model.modelParams?.effort ?? variant?.effort;
    const details = [
      effort && effort !== "default" ? effort[0].toUpperCase() + effort.slice(1) : null,
      variant?.thinking ? "Thinking" : null,
      model.modelParams?.fast || variant?.fast ? "Fast" : null,
      model.modelParams?.tier,
    ].filter(Boolean);
    return [brand, name, ...details].join(" · ");
  };
}
