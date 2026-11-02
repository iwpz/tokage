import { modelDisplay, type ModelDisplayInput } from "./modelDisplay";
import type { RuntimeSnapshot } from "./runtime";

/** Render durable failures first; older snapshots can only expose recorded policy evidence. */
export function runtimeFailure(s: RuntimeSnapshot, formatModel: (model: ModelDisplayInput) => string = model => modelDisplay(model).label) {
 // Old attempts stay in the evidence after an explicit retry. Only use them as
 // the current failure while the run is actually blocked or stopped.
 const currentAttempts=Object.values(s.execution?.tasks??{}).filter(task=>task.state!=="DONE").flatMap(task=>task.attempts.slice(-1)).map(id=>s.work?.attempts[id]).filter(a=>a!=null);
 const failedGate=currentAttempts.flatMap(a=>a.gates).find(g=>g.result?.passed===false);
 const gateError=failedGate?.result?.errorMessage??(failedGate?`${failedGate.command??failedGate.result?.command??"gate"} · exit ${failedGate.result?.exitCode??"unknown"}
${failedGate.result?.outputTail??""}`:undefined);
 const message=s.runError ?? (["Blocked","Stopped"].includes(s.state) ? s.delivery?.intent?.error ?? s.delivery?.error ?? s.planningAttempt?.error
  ?? Object.values(s.execution?.tasks??{}).find(task=>task.state==="BLOCKED"&&task.error)?.error
  ?? currentAttempts.find(attempt=>attempt.error)?.error ?? gateError
  ?? (!s.execution?Object.values(s.work?.attempts??{}).find(attempt=>attempt.error)?.error:undefined)
  ?? Object.values(s.turns??{}).find(turn=>turn.error)?.error : undefined);
 const missing=Object.values(s.turns??{}).filter(turn=>turn.ended && turn.costUsd==null);
 const unconfirmed=Object.values(s.turns??{}).filter(turn=>turn.cleanupConfirmed===false || turn.ended===false);
 const gateCleanup=Object.values(s.work?.attempts??{}).flatMap(a=>a.gates).filter(g=>g.cleanupConfirmed!==true).map(g=>g.command??g.id??"gate");
 const cleanupUnknown=s.state==="Blocked" && (unconfirmed.length>0||gateCleanup.length>0);
 const costFailure=!message || ["unknown actual turn cost blocks dispatch","unknown actual cost blocks next role"].some(reason=>message===reason || message===`invalid input: ${reason}`);
 const unknownCost=!cleanupUnknown && costFailure && s.state==="Blocked" && s.config.blockUnknownCost && missing.length>0;
 const describe=(turn:(typeof missing)[number])=>`${turn.taskId??turn.role} · ${formatModel({ provider: turn.observedProvider ?? turn.requested.provider, requestedModel: turn.observedModel ?? turn.requested.requestedModel })}`;
 return { message, cleanupUnknown, cleanupModels:[...unconfirmed.map(describe),...gateCleanup].join(", "), unknownCost, models:missing.map(describe).join(", ") };
}
