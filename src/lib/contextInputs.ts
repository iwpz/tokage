import { runtimeRetentionContext, legacyRetentionContext, type RetentionContext } from "./compactContext";
import type { ContextActionPlan } from "./ipc";
import type { ResolvedInWindowContextParams } from "./contextPlanDefaults";
import type { Conversation, ChatMessage } from "../stores/chat";
import type { RuntimeSnapshot, TranscriptEvent } from "./runtime";
import { runtimeMessage, runtimeTurns } from "./runtimeMessages";

export interface ContextObservation {
  id: string; window: string; provider: string; model: string; endedAt: number;
  completed?: boolean; contextTokens: number | null; outputTokens: number | null; apiCalls: number | null; visibleTokens: number;
}
export interface ContextInputRequest {
  sessionId?:string;role?:string;taskId?:string|null;revision?:number|null; retention?: RetentionContext | null; historyComplete?: boolean; provider: string; model: string; modelIdentity?: "CLI_OBSERVED" | "REQUESTED"; window: string; observations: ContextObservation[]; cacheAware: boolean; minSaving: number;
}
export interface ResolvedContextInputs {
  compactIntent?: import("./ipc").CompactIntent | null; compactUnavailableReason?: string | null;
  preview: ContextActionPlan; readOnly: true; cooldown: {known:boolean;turnsSinceLastCompact:number;remainingTurns:number;lastResetActionId:number|null;source:string}; params: ResolvedInWindowContextParams; sources: Record<string,string>; packVersion: string; modelId: string;
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
  return {id:m.id,window,completed:m.status==="done",provider:m.provider ?? m.ref?.provider ?? "",model:m.reportedModel ?? m.model ?? m.ref?.model ?? "",endedAt,
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
    o.completed=t.status==="Completed" && t.cleanupConfirmed && !t.error;
    const done = [...(transcripts[t.id] ?? [])].reverse().find(row=>row.event.kind==="done")?.event;
    const durable = t.usage?.source?.toUpperCase()==="ESTIMATED" ? null : t.usage;
    const receipt = done?.kind==="done" && done.source!=="ESTIMATED" ? done : null;
    o.contextTokens = durable?.contextTokens ?? receipt?.contextTokens ?? null;
    o.apiCalls = durable?.apiCalls ?? receipt?.apiCalls ?? null;
    o.outputTokens = durable?.outputTokens ?? receipt?.outputTokens ?? null;
    return o;
  });
  return {sessionId:snapshot.runId,role:last.role,taskId:last.taskId,revision:snapshot.revision,retention:runtimeRetentionContext(snapshot,last),historyComplete:compatible.length<=512,provider,model,modelIdentity:last.observedModel ? "CLI_OBSERVED" : "REQUESTED",window,observations,cacheAware,minSaving};
}
export function legacyContextInputRequest(conv: Conversation | undefined, cacheAware: boolean, minSaving: number, sessionId?: string): ContextInputRequest | null {
  const last = [...(conv?.messages ?? [])].reverse().find(m=>m.role==="assistant" && (m.status==="done" || m.status==="error" || m.status==="cancelled") && m.endedAt!=null && m.debugRoute?.output!=="mock");
  if (!conv || !last) return null;
  const model=last.reportedModel ?? last.model ?? last.ref?.model ?? "";
  const provider=last.provider ?? last.ref?.provider ?? "";
  const scoped=!!sessionId && !!last.nativeThreadId && last.nativeThreadSessionId===sessionId;
  const window=scoped ? last.nativeThreadId! : last.id;
  const messages=scoped ? conv.messages.filter(m=>m.role==="assistant" && (m.status==="done" || m.status==="error" || m.status==="cancelled") && m.endedAt!=null && m.debugRoute?.output!=="mock" && m.nativeThreadSessionId===sessionId && m.nativeThreadId===window && (m.provider ?? m.ref?.provider)===provider && (m.reportedModel ?? m.model ?? m.ref?.model)===model && m.mode===last.mode) : [last];
  const observations=messages.slice(-512).map(m=> {
    const index=conv.messages.indexOf(m);
    const o=observed(m,window,Math.floor(m.endedAt!/1000),conv.messages[index-1]?.role==="user" ? conv.messages[index-1].text : "");
    o.completed=scoped && m.status==="done" && !m.error;
    return o;
  });
  return {sessionId,role:last.mode ?? "direct",taskId:null,revision:null,retention:legacyRetentionContext(conv,last,sessionId),historyComplete:scoped && messages.length<=512 && !conv.messages.some(m=>m.role==="assistant" && m.endedAt!=null && !m.nativeThreadSessionId),provider,model,modelIdentity:last.reportedModel ? "CLI_OBSERVED" : "REQUESTED",window,observations,cacheAware,minSaving};
}
