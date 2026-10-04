import i18n from "../i18n";
import { findModel, optionSummary, recommend, refKey, resolveVariant, selectableGroups, triage, type ModelRef } from "./models";
import type { TurnMode } from "./ipc";
import { useChat, type SendArgs } from "../stores/chat";
import { useProviders } from "../stores/providers";
import { useSettings, type RequestMode } from "../stores/settings";

export type TurnPlan = Pick<SendArgs, "mode" | "auto" | "ref" | "variant" | "modelName">;

/**
 * Picks mode and model for a prompt from the current stores. Auto triages locally and takes
 * the recommendation; plan / direct honour the manual pick when it is still selectable.
 *
 * `prefer` is the turn being interrupted: when the prompt lands in the same mode, its model is
 * kept so the CLI session (and its prompt cache) continues instead of starting over elsewhere.
 * Returns null when no model can take the turn.
 */
export function planTurn(
  text: string,
  mode: RequestMode,
  prefer?: { ref: ModelRef; mode: TurnMode } | null,
): TurnPlan | null {
  const providers = useProviders.getState().providers;
  const { providerEnabled, providerTrust } = useSettings.getState();
  const { picks, modelOptions } = useChat.getState();
  const turnMode: TurnMode = mode === "auto" ? triage(text) : mode;
  const groups = selectableGroups(providers, providerEnabled, providerTrust, turnMode);
  const valid = (ref: ModelRef | null) => (ref && findModel(groups, ref) ? ref : null);
  const ref =
    (prefer && prefer.mode === turnMode ? valid(prefer.ref) : null) ??
    (mode === "auto" ? null : valid(picks[turnMode])) ??
    recommend(groups, turnMode);
  if (!ref) return null;

  const model = findModel(groups, ref)!;
  const options = modelOptions[refKey(ref)];
  const t = i18n.t.bind(i18n);
  const summary = optionSummary(
    model,
    options,
    (axis, v) =>
      axis === "effort"
        ? t(`settings.cli.efforts.${v}`, { defaultValue: v })
        : axis === "tier"
          ? t(`workspace.composer.picker.tiers.${v}`, { defaultValue: v })
          : v.toUpperCase(),
    { thinking: t("workspace.composer.picker.thinking"), fast: t("workspace.composer.picker.fast") },
  );
  return {
    mode: turnMode,
    auto: mode === "auto",
    ref,
    variant: resolveVariant(model, options),
    modelName: [model.name, ...summary].join(" · "),
  };
}
