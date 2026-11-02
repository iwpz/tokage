import type { RuntimeSnapshot } from "./runtime";
import { runtimeFailure } from "./runtimeFailure";

/** Conservative UI guidance from recorded evidence. Native admission remains authoritative.
 * No process liveness or disk health is inferred from a snapshot or a missing error.
 */
export type RecoveryKind="observation"|"storage"|"owner"|"cleanup"|"effects"|"worker"|"cancelling"|"stopped"|"review"|"gate"|"cost"|"blocked";
export function runtimeRecovery(s:RuntimeSnapshot, readError?:string, workerError?:string, commandError?:string) {
 const failure=runtimeFailure(s);
 const stopped=["Blocked","Stopped"].includes(s.state);
 const tasks=Object.entries(s.execution?.tasks??{});
 const attempts=Object.entries(s.work?.attempts??{});
 const nativeUnknown=Object.values(s.turns??{}).some(t=>!t.ended||!t.cleanupConfirmed);
 const gateUnknown=attempts.some(([,a])=>a.gates.some(g=>g.cleanupConfirmed!==true));
 const reserved=tasks.some(([,t])=>t.reservationActive===true);
 const deliveryPending=!!s.delivery?.intent&&!s.delivery.intent.completed;
 const gitPending=attempts.some(([,a])=>a.commitIntent===true&&!a.integratedHead);
 const effectsPending=nativeUnknown||gateUnknown||reserved||gitPending||deliveryPending;
 const transport=readError||workerError||commandError;
 const storageError=[transport,s.runError].some(e=>e?.includes("repository unavailable or corrupt"));
 const ownerConflict=transport?.includes("run owner conflict");
 const latest=tasks.filter(([,t])=>t.state!=="DONE").flatMap(([taskId,t])=>t.attempts.slice(-1).map(id=>({taskId,id,attempt:s.work?.attempts[id]})));
 const failedGate=latest.flatMap(a=>a.attempt?.gates??[]).some(g=>g.result?.passed===false);
 const allDone=tasks.length>0&&tasks.every(([,t])=>t.state==="DONE");
 const approvalValid=!!s.plan&&s.approvedDigest===s.plan.confirmationDigest;
 const quiet=!effectsPending&&!readError&&!workerError&&!ownerConflict&&!storageError;
 const amendmentState=stopped||!!s.execution&&["AwaitingApproval","Approved"].includes(s.state);
 const taskSafe=(id:string)=>{const task=s.execution?.tasks[id];const a=s.work?.attempts[task?.attempts.at(-1)??""];return !task?.deviation&&!a?.commitIntent&&!(a?.gates.some(g=>g.cleanupConfirmed!==true));};
 const kind:RecoveryKind|null=readError?(storageError?"storage":"observation"):ownerConflict?"owner":storageError?"storage":
  stopped&&(nativeUnknown||gateUnknown||reserved)?"cleanup":stopped&&(gitPending||deliveryPending)?"effects":
  workerError?"worker":s.state==="CancelRequested"||s.state==="Stopping"?"cancelling":s.state==="Stopped"?"stopped":
  s.state==="Blocked"&&s.delivery?.error?"review":s.state==="Blocked"&&failedGate?"gate":
  s.state==="Blocked"&&failure.unknownCost?"cost":s.state==="Blocked"?"blocked":null;
 return {kind,failure,effectsPending,latest,actions:{
  reconcile:!readError&&["Planning","Executing","Reviewing","Delivering","Blocked","Stopping","CancelRequested"].includes(s.state),
  retryRun:stopped&&quiet&&!allDone&&!!s.plan&&tasks.filter(([,t])=>t.state!=="DONE").every(([id])=>taskSafe(id)),
  review:["Blocked","AwaitingReview"].includes(s.state)&&quiet&&allDone&&approvalValid,
  amend:amendmentState&&quiet,
  retryTasks:s.state==="Blocked"&&quiet&&approvalValid?tasks.filter(([id,t])=>t.state==="BLOCKED"&&taskSafe(id)).map(([id])=>id):[],
  takeoverTasks:amendmentState&&quiet?tasks.filter(([,t])=>t.state!=="DONE"&&t.attempts.some(id=>{const a=s.work?.attempts[id];return !!a?.workspace;})).map(([id])=>id):[],
 },artifacts:attempts.map(([id,a])=>({id,...a,taskId:tasks.find(([,t])=>t.attempts.includes(id))?.[0]??null})),readError,workerError};
}
