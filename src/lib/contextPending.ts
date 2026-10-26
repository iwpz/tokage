import type { CompactIntent } from "./ipc";
import type { ContextInputRequest, ResolvedContextInputs } from "./contextInputs";

/** Explicit, session-owned acceptance. Never a global prompt or a savings baseline. */
export interface PendingCompact { intent: CompactIntent; requestKey: string; startupKey?:string; cwd?:string }
export function compactRequestKey(request: ContextInputRequest): string { return JSON.stringify(request); }
export function acceptCompact(request: ContextInputRequest, inputs: ResolvedContextInputs): PendingCompact {
  if (!inputs.cooldown.known || inputs.preview.cooldownBlocked || (inputs.preview.cooldownRemainingTurns ?? 0)>0 || inputs.preview.recommendedAction!=="COMPACT" || !inputs.compactIntent) throw new Error("Context compaction is unavailable");
  if(inputs.compactIntent.nextTurn.type==="NATIVE_COMMAND") throw new Error("Native compaction transport is not enabled");
  if(request.taskId || request.revision!=null || !["direct","plan"].includes(request.role ?? "")) throw new Error("Only ordinary main-session turns support pending compaction");
  return {intent:structuredClone(inputs.compactIntent),requestKey:compactRequestKey(request)};
}
export function validatePendingCompact(pending:PendingCompact, request:ContextInputRequest|null, provider:string, model:string, role:string):void {
  if(!request || compactRequestKey(request)!==pending.requestKey || request.provider!==provider || request.role!==role || pending.intent.scope.providerName!==provider || pending.intent.scope.role!==role || pending.intent.scope.sessionId!==request.sessionId) throw new Error("Context changed; cancel compaction and review a new suggestion");
  // Model here is the frozen requested CLI argument, not an observed alias used to choose a model.
  const last=request.observations.at(-1);
  if(!last || !model) throw new Error("Missing current model or window");
}

export function compactStartupKey(v:{id:string;effort:string|null;fast:boolean;tier:string|null}):string {return JSON.stringify([v.id,v.effort,v.fast,v.tier]);}
