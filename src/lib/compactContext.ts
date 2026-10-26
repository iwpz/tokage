import type { Conversation, ChatMessage } from "../stores/chat";
import type { RuntimeSnapshot, RuntimeTurn } from "./runtime";
import type { PlanContract } from "./ipc";

export interface RetentionContext {
  sessionId:string; turnId:string; role:string; taskId:string|null; revision:number|null;
  taskGoal:string; goalSource:string; contractPaths:string[]; contracts:PlanContract[]; pendingErrors:string[];
}
function excerptErrors(values:(string|null|undefined)[]):string[] {
  return [...new Set(values.filter((s):s is string=>typeof s==="string" && !!s.trim()).map(s=>s.slice(0,2000)))].slice(-8);
}
function failedGateErrors(gates:unknown[]):string[] {
  return gates.flatMap(g=> {
    if(!g || typeof g!=="object") return [];
    const r=(g as {result?:unknown}).result;
    if(!r || typeof r!=="object" || (r as {passed?:unknown}).passed!==false) return [];
    const result=r as {command?:unknown;errorMessage?:unknown;outputTail?:unknown};
    const error=[result.errorMessage,result.outputTail].filter((v):v is string=>typeof v==="string" && !!v.trim()).join("\n");
    return error ? [`[Gate ${typeof result.command==="string"?result.command:"failed"}] ${error}`] : [];
  }).slice(-2);
}
/** Only the selected task/window's current failures; old peer failures are not pending errors. */
export function runtimeRetentionContext(s:RuntimeSnapshot,last:RuntimeTurn):RetentionContext|null {
  if(Object.values(s.turns ?? {}).some(t=>!t.ended && t.role===last.role && t.taskId===last.taskId)) return null;
  const task=last.taskId ? s.plan?.plan.tasks.find(t=>t.id===last.taskId) : null;
  if(last.taskId && !task) return null;
  const taskGoal=task ? task.goal : s.plan?.plan.spec.goal || s.requirement;
  if(!taskGoal.trim()) return null;
  const contracts=task ? task.contracts.map(id=>s.plan?.plan.contracts.find(c=>c.id===id)) : s.plan?.plan.contracts ?? [];
  if(contracts.some(c=>!c)) return null;
  const state=last.taskId ? s.execution?.tasks[last.taskId] : null;
  const currentTaskFailure=state && ["BLOCKED","INTERRUPTED","FAILED","REPAIRING"].includes(state.state.toUpperCase()) && state.attempts.includes(last.id);
  const currentTurnFailure=last.error || ["FAILED","STOPPED","INTERRUPTED"].includes(last.status.toUpperCase());
  const mainBlocked=!last.taskId && ["Blocked","Stopped"].includes(s.state);
  return {sessionId:s.runId,turnId:last.id,role:last.role,taskId:last.taskId,revision:s.revision,
    taskGoal,goalSource:task?"ACTIVE_TASK_GOAL":s.plan?.plan.spec.goal?"CURRENT_PLAN_SPEC_GOAL":"RUN_REQUIREMENT",
    // PlanContract has embedded content but no physical file path. Never invent one.
    contractPaths:[],contracts:contracts as PlanContract[],
    pendingErrors:excerptErrors([last.error,currentTaskFailure?state?.error:null,(currentTurnFailure||currentTaskFailure)?s.work?.attempts[last.id]?.error:null,mainBlocked?s.runError:null,mainBlocked&&last.role==="planner"?s.planningAttempt?.error:null,mainBlocked&&last.role==="reviewer"?s.delivery?.error:null,...((currentTurnFailure||currentTaskFailure)?failedGateErrors(s.work?.attempts[last.id]?.gates ?? []):[])])};
}
export function legacyRetentionContext(conv:Conversation,last:ChatMessage,sessionId?:string):RetentionContext|null {
  if(!sessionId || !last.nativeThreadId || !last.mode || !conv.thread || conv.thread.id!==last.nativeThreadId || conv.thread.ref.provider!==(last.ref?.provider ?? last.provider) || conv.thread.ref.model!==(last.ref?.model ?? last.model) || (conv.thread.mode && conv.thread.mode!==last.mode) || last.nativeThreadSessionId!==sessionId || conv.messages.some(m=>m.status==="streaming" || m.status==="waiting_approval")) return null;
  const first=conv.messages.find(m=>m.role==="user" && m.text.trim());
  if(!first) return null;
  const scoped=conv.messages.filter(m=>m.role==="assistant" && m.nativeThreadId===last.nativeThreadId && m.nativeThreadSessionId===sessionId && (m.provider ?? m.ref?.provider)===(last.provider ?? last.ref?.provider) && (m.reportedModel ?? m.model)===(last.reportedModel ?? last.model) && m.mode===last.mode);
  const lastSuccess=scoped.map(m=>m.status).lastIndexOf("done");
  return {sessionId,turnId:last.id,role:last.mode ?? "direct",taskId:null,revision:null,taskGoal:first.text,goalSource:"SESSION_FIRST_USER_MESSAGE",contractPaths:[],contracts:[],pendingErrors:excerptErrors([last.error,...scoped.slice(lastSuccess+1).map(m=>m.error)])};
}
