import type { RuntimeSnapshot } from "./runtime";

/** Render durable failures first; older snapshots can only expose recorded policy evidence. */
export function runtimeFailure(s: RuntimeSnapshot) {
 const message=s.runError ?? s.delivery?.intent?.error ?? s.delivery?.error ?? s.planningAttempt?.error
  ?? Object.values(s.execution?.tasks??{}).find(task=>task.error)?.error
  ?? Object.values(s.work?.attempts??{}).find(attempt=>attempt.error)?.error
  ?? Object.values(s.turns??{}).find(turn=>turn.error)?.error;
 const missing=Object.values(s.turns??{}).filter(turn=>turn.ended && turn.costUsd==null);
 const unconfirmed=Object.values(s.turns??{}).filter(turn=>turn.cleanupConfirmed===false || turn.ended===false);
 const cleanupUnknown=s.state==="Blocked" && unconfirmed.length>0;
 const costFailure=!message || ["unknown actual turn cost blocks dispatch","unknown actual cost blocks next role"].some(reason=>message===reason || message===`invalid input: ${reason}`);
 const unknownCost=!cleanupUnknown && costFailure && s.state==="Blocked" && s.config.blockUnknownCost && missing.length>0;
 const describe=(turn:(typeof missing)[number])=>`${turn.taskId??turn.role} · ${turn.observedModel??turn.requested.requestedModel}`;
 return { message, cleanupUnknown, cleanupModels:unconfirmed.map(describe).join(", "), unknownCost, models:missing.map(describe).join(", ") };
}
