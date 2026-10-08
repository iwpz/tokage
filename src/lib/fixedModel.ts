/**
 * Single-model (fixed) mode helpers: lock one model for planning + every execution path.
 */
import type { Plan } from "./ipc";
import {
  findModel,
  refKey,
  resolveVariant,
  selectableGroups,
  type ModelRef,
} from "./models";
import { useChat } from "../stores/chat";
import { useProviders } from "../stores/providers";
import { useSettings, type RequestMode } from "../stores/settings";

export function isFixedMode(mode: RequestMode): boolean {
  return mode === "fixed";
}

/** `ProviderId:cliModelArg` for allocation overrides / assigned_model. */
export function modelSpecFromRef(ref: ModelRef): string {
  const providers = useProviders.getState().providers;
  const { providerEnabled, providerTrust, blockedModels, modelBanningLevel, modelProviderPreferences } =
    useSettings.getState();
  const groups = selectableGroups(
    providers,
    providerEnabled,
    providerTrust,
    "direct",
    blockedModels,
    modelBanningLevel,
    modelProviderPreferences,
  );
  const model = findModel(groups, ref);
  if (!model) return `${ref.provider}:${ref.model}`;
  const options = useChat.getState().modelOptions[refKey(ref)];
  const variant = resolveVariant(model, options);
  return `${ref.provider}:${variant.id}`;
}

export function sessionLockedModel(sessionId: string | null | undefined): ModelRef | null {
  if (!sessionId) return null;
  return useChat.getState().conversations[sessionId]?.lockedModel ?? null;
}

/** Force every task onto the locked model (single-model mode). */
export function fixedModelOverrides(plan: Plan, ref: ModelRef): Record<string, string> {
  const spec = modelSpecFromRef(ref);
  const overrides: Record<string, string> = {};
  for (const t of plan.tasks) {
    overrides[t.id] = spec;
  }
  return overrides;
}
