import type { Conversation, ChatMessage } from "../stores/chat";
import type { RuntimeSnapshot, RuntimeTurn } from "./runtime";
import type { PlanContract } from "./ipc";

export interface GeneratedProgressReport {
  sourceTurnId:string; reportTurnId:string; costUsd:number|null; baselineParams:import("./contextPlanDefaults").ResolvedInWindowContextParams;
}
export interface ProgressContext {
  generatedReport?:GeneratedProgressReport|null; sourceTurnIds:string[]; localReport:string; modelReport?:string|null; changedFiles:string[];
}
export interface RetentionContext {
  sessionId:string; turnId:string; role:string; taskId:string|null; revision:number|null;
  taskGoal:string; goalSource:string; contractPaths:string[]; contracts:PlanContract[]; pendingErrors:string[]; progress?:ProgressContext|null;
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
  const lastSuccess=scoped.map(m=>m.contextReport ? undefined : m.status).lastIndexOf("done");
  return {sessionId,turnId:last.id,role:last.mode ?? "direct",taskId:null,revision:null,taskGoal:first.text,goalSource:"SESSION_FIRST_USER_MESSAGE",contractPaths:[],contracts:[],pendingErrors:excerptErrors([last.error,...scoped.slice(lastSuccess+1).map(m=>m.error)]),progress:legacyProgressContext(conv,scoped)};
}

/** Bounded excerpts, not a claim that edits passed tests or that a model summarized history. */
export function legacyProgressContext(conv:Conversation, scoped:ChatMessage[]):ProgressContext {
  const recent=scoped.slice(-8);
  const last=recent.at(-1)!;
  const clip=(text:string,max:number)=>Array.from(text).slice(0,max).join("");
  const changedFiles=[...new Set(recent.flatMap(m=>(m.files ?? []).map(f=>f.path)))].slice(0,32).map(p=>clip(p,256));
  const index=conv.messages.indexOf(last);
  const user=conv.messages[index-1]?.role==="user" ? conv.messages[index-1].text : "";
  const localReport=`# Locally extracted progress (unverified excerpts)\nChanged paths: ${changedFiles.length?changedFiles.slice(0,8).join(", "):"None recorded"}\nRecent user request: ${clip(user,800)}\nLatest assistant output: ${clip(last.text,2400)}\nRecent errors: ${excerptErrors(recent.map(m=>m.error)).slice(-2).map(e=>clip(e,200)).join("; ") || "None recorded"}\nNext step: follow the current user message; verify excerpts against source before acting.`;
  const headers=["What was completed","Deviations from contracts","Hints for downstream","Self-check against acceptance criteria","Next step"];
  // Reuse only a complete, already generated report from this exact source turn.
  // Reasoning/tool output and foreign windows are never promoted to a model report.
  const positions=headers.map(h=>last.text.search(new RegExp(`^#{1,4} ${h}[^\\n]*$`,"m")));
  const complete=last.status==="done" && !last.error && positions.every((n,i)=>{
    if(n<0 || (i>0 && n<=positions[i-1])) return false;
    const newline=last.text.indexOf("\n",n);
    return newline>=0 && !!last.text.slice(newline+1,positions[i+1] ?? last.text.length).trim();
  });
  const report=complete?last.text.slice(positions[0]).trim():"";
  const generatedReport=last.contextReport && last.status==="done" && !last.error
    ? {sourceTurnId:last.contextReport.sourceTurnId,reportTurnId:last.id,costUsd:last.usage?.costUsd ?? null,baselineParams:last.contextReport.baselineParams} : null;
  return {generatedReport,sourceTurnIds:recent.map(m=>m.id),changedFiles,localReport:clip(localReport,6000),modelReport:report && Array.from(report).length<=6000?report:null};
}

/** One ordinary request in the existing window, not an instruction to change files. */
export const PROGRESS_REPORT_REQUEST = "Produce a concise progress report for handing this task to a new window. Do not edit files, run commands or start new work. Use only what this window already knows; mark missing evidence as unknown. Use exactly these headings in this order, with non-empty content under each: ### What was completed; ### Deviations from contracts; ### Hints for downstream; ### Self-check against acceptance criteria; ### Next step. Write in the task language, keep the English headings, and stay within 300 tokens. Do not claim tests passed unless this window has their results.";
