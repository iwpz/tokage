import {findModel, recommend, type ModelGroup, type ModelRef} from "./models";
import type {Conversation} from "../stores/chat";
import type {SettingsState} from "../stores/settings";

export interface DirectSelection {
  ref: ModelRef | null;
  reason: "manual" | "continue" | "recommended" | "unavailable";
}

/** One local decision for both the visible preview and the actual dispatch.
 * A missing selected/previous resource never authorizes silent failover. */
export function selectDirectResource(
  groups: ModelGroup[],
  conversation: Conversation | undefined,
  initialPick: ModelRef | null,
  settings: Pick<SettingsState,"modelProviderPreferences"|"modelChannelAccess">,
): DirectSelection {
  const previous = [...(conversation?.messages ?? [])].reverse().find(m=>m.role==="assistant" && m.mode==="direct" && !m.contextReport)?.ref
    ?? (conversation?.thread?.mode!=="plan" ? conversation?.thread?.ref : undefined);
  const manual = conversation && "directModel" in conversation ? conversation.directModel : previous ? null : initialPick;
  const selected = manual ?? previous;
  if(selected) return findModel(groups,selected)
    ? {ref:selected,reason:manual?"manual":"continue"}
    : {ref:null,reason:"unavailable"};
  return {ref:recommend(groups,"direct",settings.modelProviderPreferences,settings.modelChannelAccess),reason:"recommended"};
}
