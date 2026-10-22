import { useExecution } from "../stores/execution";
/** Compatibility boundary. New durable runs cannot enter the retired frontend engine. */
import * as legacy from "./legacyTaskRunner";
import { inTauri } from "./ipc";
import { demoIdentity } from "./demoIdentity";
import { useSettings } from "../stores/settings";
import { runtimeSnapshot } from "./runtime";
import { useRuntime } from "../stores/runtime";
export type { RunningSubAgentContext, ResolvedModel, TaskRunnerOptions, GateRunContext, SupplementResult } from "./legacyTaskRunner";
export {
 syncMainSessionState,onProviderMeltedEvent,onProviderRecoveredEvent,
 getRunningSubAgent,registerRunningSubAgent,unregisterRunningSubAgent,
 transitiveDownstreamTaskIds,formatAgentTimeoutBlockReason,formatTaskWaitReason,
 ensureSubAgentEventListener,resetSubAgentEventListener,handleSubAgentChatEnded,handleSubAgentChatEvent,
 resolveTaskProviderAndModel,buildSubAgentPrompt,getTaskTerminalResumeCommand,
} from "./legacyTaskRunner";
function usesSharedTransport(sessionId:string){return inTauri&&!demoIdentity(sessionId)&&useSettings.getState().debugProvider.output!=="mock";}
function legacyCancelKnown(sessionId:string){
 if(!usesSharedTransport(sessionId))return true;
 if(useRuntime.getState().runs?.[sessionId])return false;
 const old=useExecution.getState().pipelines[sessionId];
 return !!old&&!old.delivery?.phase?.match(/^(merged|exported|kept)$/);
}
async function shared(sessionId:string){
 if(!usesSharedTransport(sessionId))return null;
 return runtimeSnapshot(sessionId); // Query errors fail closed; never fall back to the legacy engine.
}
function legacyOnly<A extends unknown[],R>(fn:(...args:A)=>Promise<R>,session:(args:A)=>string){
 return (...args:A):Promise<R>=>{const id=session(args);if(!usesSharedTransport(id))return fn(...args);return shared(id).then(run=>{if(run)throw new Error("This run is owned by runtime; use its explicit controls.");return fn(...args);});};
}
export const applyRepairLadder=legacyOnly(legacy.applyRepairLadder,a=>a[0].sessionId);
export const dispatchSubAgentTask=legacyOnly(legacy.dispatchSubAgentTask,a=>a[0].sessionId);
export const ensureSessionIntegration=legacyOnly(legacy.ensureSessionIntegration,a=>a[0]);
export const runGatesForTask=legacyOnly(legacy.runGatesForTask,a=>a[0].sessionId);
export const completeTakeoverAndRerunGates=legacyOnly(legacy.completeTakeoverAndRerunGates,a=>a[0]);
export const processSubAgentCompletion=legacyOnly(legacy.processSubAgentCompletion,a=>a[0].sessionId);
export const progressDownstreamTasks=legacyOnly(legacy.progressDownstreamTasks,a=>a[0]);
export const maybeStartPlanDelivery=legacyOnly(legacy.maybeStartPlanDelivery,a=>a[0]);
export const dispatchReadyTasks=legacyOnly(legacy.dispatchReadyTasks,a=>a[0]);
export const confirmContractDeviation=legacyOnly(legacy.confirmContractDeviation,a=>a[0]);
export const resumePipelineTaskAfterApproval=legacyOnly(legacy.resumePipelineTaskAfterApproval,a=>a[0]);
export const deliverMergeToCheckout=legacyOnly(legacy.deliverMergeToCheckout,a=>a[0]);
export const deliverExportPatch=legacyOnly(legacy.deliverExportPatch,a=>a[0]);
export const deliverKeepBranch=legacyOnly(legacy.deliverKeepBranch,a=>a[0]);
export async function continueTask(...args:Parameters<typeof legacy.continueTask>){if(!usesSharedTransport(args[0]))return legacy.continueTask(...args);if(await shared(args[0])){await useRuntime.getState().command(args[0],{type:"RetryTask",payload:{taskId:args[1]}});return;}return legacy.continueTask(...args);}
export async function cancelTask(...args:Parameters<typeof legacy.cancelTask>){if(legacyCancelKnown(args[0]))return legacy.cancelTask(...args);if(await shared(args[0])){await useRuntime.getState().command(args[0],{type:"CancelRun"});return;}return legacy.cancelTask(...args);}
export async function interruptExecution(...args:Parameters<typeof legacy.interruptExecution>){if(legacyCancelKnown(args[0]))return legacy.interruptExecution(...args);if(await shared(args[0])){await useRuntime.getState().command(args[0],{type:"CancelRun"});return;}return legacy.interruptExecution(...args);}
export async function cancelPlanAcceptance(...args:Parameters<typeof legacy.cancelPlanAcceptance>){if(legacyCancelKnown(args[0]))return legacy.cancelPlanAcceptance(...args);if(await shared(args[0])){await useRuntime.getState().command(args[0],{type:"CancelRun"});return;}return legacy.cancelPlanAcceptance(...args);}
export async function continueInterrupted(...args:Parameters<typeof legacy.continueInterrupted>){if(!usesSharedTransport(args[0]))return legacy.continueInterrupted(...args);if(await shared(args[0])){await useRuntime.getState().command(args[0],{type:"ResumeRun"});return;}return legacy.continueInterrupted(...args);}
export async function rerunPlanAcceptance(...args:Parameters<typeof legacy.rerunPlanAcceptance>){if(!usesSharedTransport(args[0]))return legacy.rerunPlanAcceptance(...args);if(await shared(args[0])){await useRuntime.getState().command(args[0],{type:"ReviewDelivery"});return;}return legacy.rerunPlanAcceptance(...args);}
export async function supplementTask(...args:Parameters<typeof legacy.supplementTask>):Promise<legacy.SupplementResult>{if(!usesSharedTransport(args[0]))return legacy.supplementTask(...args);const s=await shared(args[0]);if(s){await useRuntime.getState().command(args[0],{type:"SupplementTask",payload:{taskId:args[1],text:args[2],confirmationDigest:s.plan?.confirmationDigest}});return "updated";}return legacy.supplementTask(...args);}
export async function resolvePipelineTaskApproval(...args:Parameters<typeof legacy.resolvePipelineTaskApproval>){
 if(!usesSharedTransport(args[0]))return legacy.resolvePipelineTaskApproval(...args);
 const s=await shared(args[0]);if(!s)return legacy.resolvePipelineTaskApproval(...args);
 const turn=Object.values(s.turns??{}).find(t=>t.taskId===args[1]&&t.approval?.id===args[2]);
 if(!turn?.approval)throw new Error("Approval is stale");
 const decision={allow_once:"AllowOnce",allow_session:"AllowSession",deny:"Deny"}[args[3] as string];
 if(!decision)throw new Error("This approval scope is unsupported");
 await useRuntime.getState().command(args[0],{type:"ResolveApproval",payload:{turnId:turn.id,approvalId:turn.approval.id,confirmationDigest:turn.approval.confirmationDigest,decision}});
}
