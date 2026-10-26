import type { ContextActionPlan, InWindowContextParams } from "./ipc";
import type { Conversation, ChatMessage } from "../stores/chat";
import type { RuntimeSnapshot, TranscriptEvent } from "./runtime";
import { runtimeMessage, runtimeTurns } from "./runtimeMessages";

export interface ContextObservation {
  id: string; window: string; provider: string; model: string; endedAt: number;
  contextTokens: number | null; outputTokens: number | null; apiCalls: number | null; visibleTokens: number;
}
export interface ContextInputRequest {
  provider: string; model: string; modelIdentity?: "CLI_OBSERVED" | "REQUESTED"; window: string; observations: ContextObservation[]; cacheAware: boolean; minSaving: number;
}
export interface ResolvedContextInputs {
  preview: ContextActionPlan; readOnly: true; params: InWindowContextParams; sources: Record<string,string>; packVersion: string; modelId: string;
  totalTokens: number; sampleCount: number; warmNow: boolean; cacheTtlRemainingSeconds: number;
}
/** Visible-content estimate only; CLI system/tool prompts may be unavailable. */
export function estimateVisibleTokens(text: string): number {
  let weight = 0;
  for (const char of text) weight += char.codePointAt(0)! <= 0x7f ? 0.25 : 0.5;
  return Math.ceil(weight);
}
function observed(m: ChatMessage, window: string, endedAt: number, extra = ""): ContextObservation {
  // Aggregate input/cache usage is deliberately excluded: it can span many API calls.
  const visible = extra + m.text + (m.parts ?? []).filter(p=>p.type==="tool").map(p=>JSON.stringify(p)).join("");
  return {id:m.id,window,provider:m.provider ?? m.ref?.provider ?? "",model:m.reportedModel ?? m.model ?? m.ref?.model ?? "",endedAt,
    contextTokens:m.usage?.source==="ESTIMATED" ? null : m.usage?.contextTokens ?? null,outputTokens:m.usage?.source==="ESTIMATED" ? null : m.usage?.output ?? null,apiCalls:m.usage?.source==="ESTIMATED" ? null : m.usage?.apiCalls ?? null,visibleTokens:estimateVisibleTokens(visible)};
}
export function runtimeContextInputRequest(snapshot: RuntimeSnapshot, transcripts: Record<string,TranscriptEvent[]>, order: readonly string[], cacheAware: boolean, minSaving: number): ContextInputRequest | null {
  const turns = runtimeTurns(snapshot,order).filter(t=>t.ended && t.finishedAt!=null).sort((a,b)=>a.finishedAt!-b.finishedAt!);
  const last = turns.at(-1);
  if (!last) return null;
  const window = last.threadId ?? last.id;
  const provider = last.observedProvider ?? last.requested.provider, model = last.observedModel ?? last.requested.requestedModel;
  // Only this role/task/thread; parallel tasks are never one history window.
  const compatible = turns.filter(t=>(t.threadId ?? t.id)===window && t.requested.provider===last.requested.provider && t.requested.requestedModel===last.requested.requestedModel && (t.observedProvider ?? t.requested.provider)===provider && (t.observedModel ?? t.requested.requestedModel)===model && t.role===last.role && t.taskId===last.taskId);
  const observations = compatible.slice(-512).map((t,index)=> {
    const m = runtimeMessage(snapshot,t,transcripts[t.id] ?? []);
    const o = observed(m,window,t.finishedAt!,t.role==="planner" && index===0 ? snapshot.requirement : "");
    o.provider=provider;
    const done = [...(transcripts[t.id] ?? [])].reverse().find(row=>row.event.kind==="done")?.event;
    const durable = t.usage?.source?.toUpperCase()==="ESTIMATED" ? null : t.usage;
    const receipt = done?.kind==="done" && done.source!=="ESTIMATED" ? done : null;
    o.contextTokens = durable?.contextTokens ?? receipt?.contextTokens ?? null;
    o.apiCalls = durable?.apiCalls ?? receipt?.apiCalls ?? null;
    o.outputTokens = durable?.outputTokens ?? receipt?.outputTokens ?? null;
    return o;
  });
  return {provider,model,modelIdentity:last.observedModel ? "CLI_OBSERVED" : "REQUESTED",window,observations,cacheAware,minSaving};
}
export function legacyContextInputRequest(conv: Conversation | undefined, cacheAware: boolean, minSaving: number): ContextInputRequest | null {
  // Legacy messages do not carry a durable per-message native thread identity.
  // Use only the last completed assistant rather than joining unrelated windows.
  const last = [...(conv?.messages ?? [])].reverse().find(m=>m.role==="assistant" && m.status==="done" && m.endedAt!=null && m.debugRoute?.output!=="mock");
  const index = last && conv ? conv.messages.indexOf(last) : -1;
  if (!conv || index<0) return null;
  const message = conv.messages[index];
  const o = observed(message,message.id,Math.floor(message.endedAt!/1000),conv.messages[index-1]?.role==="user" ? conv.messages[index-1].text : "");
  return {provider:o.provider,model:o.model,modelIdentity:message.reportedModel ? "CLI_OBSERVED" : "REQUESTED",window:o.window,observations:[o],cacheAware,minSaving};
}
