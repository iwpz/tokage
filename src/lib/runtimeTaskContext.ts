import type { RuntimeContextPreparation, RuntimeSnapshot, RuntimeTurn } from "./runtime";
/** Projection only: a carrier finishing is never a native compaction receipt. */
export function taskContextHistory(s:RuntimeSnapshot, taskId:string) {
 const task=s.execution?.tasks[taskId];
 const turns=(task?.attempts??[]).map(id=>s.turns?.[id]).filter((v):v is RuntimeTurn=>!!v&&v.taskId===taskId&&v.role==="executor");
 const latest=turns.at(-1);
 const current=latest?.threadId?latest: [...turns].reverse().find(t=>t.threadId);
 const inWindow=(t:RuntimeTurn|undefined)=>!!current&&!!t&&t.taskId===taskId&&t.role==="executor"&&!!current.windowOrigin&&t.windowOrigin===current.windowOrigin&&t.threadId===current.threadId&&t.requested.provider===current.requested.provider&&t.requested.requestedModel===current.requested.requestedModel&&t.observedProvider===current.observedProvider&&t.observedModel===current.observedModel;
 const compatibleTarget=(t:RuntimeTurn)=>!!current&&t.requested.provider===current.requested.provider&&t.requested.requestedModel===current.requested.requestedModel&&(!t.observedProvider||t.observedProvider===current.observedProvider)&&(!t.observedModel||t.observedModel===current.observedModel);
 const rows:{preparation:RuntimeContextPreparation;turn?:RuntimeTurn;stale?:boolean}[]=[];
 for(const turn of turns){
  const p=turn.contextPreparation;
  // Include the FRESH action that entered this window, plus its outgoing actions.
  if(p&&(inWindow(turn)||compatibleTarget(turn)&&inWindow(s.turns?.[p.sourceTurn])))rows.push({preparation:p,turn});
 }
 for(const p of task?.contextActions??[]){
  if(turns.some(t=>t.id===p.targetTurn&&t.contextPreparation?.sourceTurn===p.sourceTurn&&t.contextPreparation.userAccepted))continue;
  const stale=p.status==="PREPARED"&&(p.sourceTurn!==latest?.id||p.configHash!==s.configHash||p.planDigest!==s.approvedDigest);
  if(inWindow(s.turns?.[p.sourceTurn])||p.status==="PREPARED")rows.push({preparation:p,stale});
 }
 rows.sort((a,b)=>a.preparation.sourceRevision-b.preparation.sourceRevision||(a.preparation.boundAt??0)-(b.preparation.boundAt??0));
 return {source:latest,current,rows};
}
export function contextCarrierState(turn?:RuntimeTurn):"pending"|"completed"|"failed" {
 return !turn?.ended?"pending":turn.status==="Completed"&&turn.cleanupConfirmed?"completed":"failed";
}
