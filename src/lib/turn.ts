import i18n from "../i18n";
import { currentAvailableModels, findModel, optionSummary, recommend, refKey, resolveVariant, selectableGroups, triage, type ModelRef } from "./models";
import { isFixedMode } from "./fixedModel";
import {
  assignDirectTask,
  calculateDirectCounterfactualCost,
  getProviderUtilizationNow,
  recordSavingsEvent,
  triageRequest,
  type TaskSize,
  type TurnMode,
} from "./ipc";
import { useChat, type SendArgs } from "../stores/chat";
import { useProviders } from "../stores/providers";
import { useSettings, type RequestMode } from "../stores/settings";

export type TurnPlan = Pick<SendArgs, "mode" | "auto" | "ref" | "variant" | "modelName"> & {
  /** F16.d: pending DIRECT savings baseline when the turn finishes. */
  directSavings?: { baselineUsd: number; domain: string };
};

function buildTurnPlan(
  turnMode: TurnMode,
  auto: boolean,
  ref: ModelRef,
): TurnPlan {
  const providers = useProviders.getState().providers;
  const {
    providerEnabled,
    providerTrust,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
  } = useSettings.getState();
  const groups = selectableGroups(
    providers,
    providerEnabled,
    providerTrust,
    turnMode,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
  );
  // Fixed mode may pick a model only listed under direct; fall back to direct groups.
  const model =
    findModel(groups, ref) ??
    findModel(
      selectableGroups(
        providers,
        providerEnabled,
        providerTrust,
        "direct",
        blockedModels,
        modelBanningLevel,
        modelProviderPreferences,
        modelChannelAccess,
      ),
      ref,
    );
  if (!model) {
    // Caller must have validated; keep a safe shell so TypeScript is happy.
    return {
      mode: turnMode,
      auto,
      ref,
      variant: { id: ref.model, effort: null, fast: false, tier: null },
      modelName: ref.model,
    };
  }
  const options = useChat.getState().modelOptions[refKey(ref)];
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
    auto,
    ref,
    variant: resolveVariant(model, options),
    modelName: [model.name, ...summary].join(" · "),
  };
}

/**
 * Resolves the locked model for single-model mode from the composer pick (or recommendation).
 */
function resolveFixedRef(): ModelRef | null {
  const providers = useProviders.getState().providers;
  const {
    providerEnabled,
    providerTrust,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
  } = useSettings.getState();
  const { picks } = useChat.getState();
  const groups = selectableGroups(
    providers,
    providerEnabled,
    providerTrust,
    "direct",
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
  );
  const valid = (ref: ModelRef | null) => (ref && findModel(groups, ref) ? ref : null);
  return valid(picks.fixed) ?? recommend(groups, "direct", modelProviderPreferences, modelChannelAccess);
}

/**
 * Picks mode and model for a prompt from the current stores. Auto triages locally and takes
 * the recommendation; plan / direct honour the manual pick when it is still selectable.
 * Fixed mode locks one model for both PLAN and DIRECT routes (triage still chooses the route).
 *
 * `prefer` is the turn being interrupted: when the prompt lands in the same mode, its model is
 * kept so the CLI session (and its prompt cache) continues instead of starting over elsewhere.
 * Returns null when no model can take the turn.
 */
export function planTurn(
  text: string,
  mode: RequestMode,
  prefer?: { ref: ModelRef; mode: TurnMode } | null,
  explicitRoute?: TurnMode | null,
): TurnPlan | null {
  if (isFixedMode(mode)) {
    const fixedRef = resolveFixedRef();
    if (!fixedRef) return null;
    const turnMode: TurnMode = explicitRoute ?? triage(text);
    // Prefer continuing the same CLI session when the locked model matches.
    const ref =
      prefer && prefer.ref.provider === fixedRef.provider && prefer.ref.model === fixedRef.model
        ? prefer.ref
        : fixedRef;
    return buildTurnPlan(turnMode, false, ref);
  }

  const providers = useProviders.getState().providers;
  const {
    providerEnabled,
    providerTrust,
    blockedModels,
    plannerModel,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
  } = useSettings.getState();
  const { picks, modelOptions } = useChat.getState();
  // Fixed returns above; remaining modes are auto | plan | direct.
  const turnMode: TurnMode =
    explicitRoute ?? (mode === "plan" || mode === "direct" ? mode : triage(text));
  const groups = selectableGroups(
    providers,
    providerEnabled,
    providerTrust,
    turnMode,
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
    modelChannelAccess,
  );
  const valid = (ref: ModelRef | null) => (ref && findModel(groups, ref) ? ref : null);

  // F16.f: plannerModel override for Plan-mode first picks (not "auto").
  let plannerPick: ModelRef | null = null;
  if (turnMode === "plan" && plannerModel && plannerModel !== "auto") {
    for (const g of groups) {
      const m = g.models.find(
        (x) =>
          x.key === plannerModel ||
          x.name === plannerModel ||
          x.variants.some((v) => v.id === plannerModel),
      );
      if (m) {
        plannerPick = { provider: g.provider.id, model: m.key };
        break;
      }
    }
  }

  const ref =
    (prefer && prefer.mode === turnMode ? valid(prefer.ref) : null) ??
    (turnMode === "plan" ? valid(plannerPick) : null) ??
    (mode === "auto" ? null : valid(picks[turnMode])) ??
    recommend(groups, turnMode, modelProviderPreferences, modelChannelAccess);
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

/**
 * Asynchronously plans turn by calling backend triageRequest when in auto mode.
 * F16.d: DIRECT route uses assignDirectTask to pick the model.
 * Fixed mode: triage route only; model always from the locked pick (skips matching).
 */
export async function planTurnAsync(
  text: string,
  mode: RequestMode,
  projectPath?: string,
  prefer?: { ref: ModelRef; mode: TurnMode } | null,
): Promise<TurnPlan | null> {
  let explicitRoute: TurnMode | null = null;
  let detectedDomain = "general";
  let difficulty = 2;

  if (mode === "auto" || isFixedMode(mode)) {
    try {
      const res = await triageRequest(text, projectPath);
      explicitRoute = res.route === "PLAN" ? "plan" : "direct";
      detectedDomain = res.detectedDomains?.[0] || "general";
      const scoreSpread = (res.scorePlan ?? 0) - (res.scoreDirect ?? 0);
      difficulty = scoreSpread > 1 ? 3 : scoreSpread > 0 ? 2 : 1;
    } catch {
      explicitRoute = triage(text);
    }
  }

  if (isFixedMode(mode)) {
    return planTurn(text, mode, prefer, explicitRoute);
  }

  if (explicitRoute === "direct" || (mode === "direct" && !prefer)) {
    try {
      const utils: Record<string, number> = {};
      for (const p of useProviders.getState().providers) {
        if (p.status === "NOT_INSTALLED") continue;
        try {
          const u = await getProviderUtilizationNow(p.id, 0);
          utils[p.id] = (u.utilizationPct ?? 0) / 100;
        } catch {
          /* skip */
        }
      }
      const size: TaskSize = text.length > 800 ? "L" : text.length > 200 ? "M" : "S";
      const assignment = await assignDirectTask(detectedDomain, difficulty, size, utils, currentAvailableModels());
      if (assignment) {
        const providers = useProviders.getState().providers;
        const {
          providerEnabled,
          providerTrust,
          blockedModels,
          modelBanningLevel,
          modelProviderPreferences,
          modelChannelAccess,
        } = useSettings.getState();
        const groups = selectableGroups(
          providers,
          providerEnabled,
          providerTrust,
          "direct",
          blockedModels,
          modelBanningLevel,
          modelProviderPreferences,
          modelChannelAccess,
        );
        const ref: ModelRef = {
          provider: assignment.provider as ModelRef["provider"],
          model: assignment.cliModelArg,
        };
        const model = findModel(groups, ref);
        if (model) {
          const refModel = useSettings.getState().referenceModel;
          const baselineUsd = await calculateDirectCounterfactualCost({
            domain: detectedDomain,
            difficulty,
            size,
            referenceModel: refModel && refModel !== "auto" ? refModel : undefined,
          }).catch(() => assignment.estCostUsd * 2);
          return {
            mode: "direct",
            auto: mode === "auto",
            ref,
            variant: resolveVariant(model, useChat.getState().modelOptions[refKey(ref)]),
            modelName: assignment.displayName,
            directSavings: { baselineUsd, domain: detectedDomain },
          };
        }
      }
    } catch (err) {
      console.warn("[turn] assignDirectTask failed, falling back:", err);
    }
  }

  return planTurn(text, mode, prefer, explicitRoute);
}

/** F16.d: record DIRECT savings after a successful direct turn. */
export async function settleDirectSavings(
  sessionId: string,
  direct: { baselineUsd: number; domain: string },
  actualCostUsd: number | null | undefined,
): Promise<void> {
  try {
    await recordSavingsEvent({
      sessionId,
      kind: "DIRECT",
      refId: direct.domain,
      baselineUsd: direct.baselineUsd,
      actualUsd: actualCostUsd ?? 0,
      detailJson: JSON.stringify({ domain: direct.domain }),
    });
  } catch (err) {
    console.warn("[turn] record DIRECT savings failed:", err);
  }
}
