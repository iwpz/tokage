import { invoke } from "@tauri-apps/api/core";
import type { Plan } from "../../lib/ipc";
import { RuntimePlanReview } from "./RuntimePlanReview";
import { RuntimeTaskBoard } from "./RuntimeTaskBoard";
import { RuntimeHistoryView } from "./RuntimeHistoryView";
import { RuntimeConversation } from "./RuntimeConversation";
import type { WorkspaceTab } from "../../stores/ui";
import { useProjects } from "../../stores/projects";
import { runtimeInspectTask, type TaskInspection } from "../../lib/runtime";
import { openPathIn } from "../../lib/ipc";
import { useEffect,useState } from "react";
import { useTranslation } from "react-i18next";
import { observeRuntime,useRuntime } from "../../stores/runtime";
import { useAuth } from "../../stores/auth";
import { Button } from "../ui";
export function RuntimeView({sessionId,view="pipeline",onSwitchTab}:{sessionId:string;view?:"chat"|"plan"|"pipeline"|"session";onSwitchTab?:(tab:WorkspaceTab)=>void}){
 const {t}=useTranslation();const projection=useRuntime(s=>s.runs[sessionId]);const observationError=useRuntime(s=>s.errors[sessionId]);
 const [error,setError]=useState("");const [busy,setBusy]=useState(false);const [text,setText]=useState("");const [confirmed,setConfirmed]=useState<string[]>([]);const [output,setOutput]=useState("");
 const [modelPicks,setModelPicks]=useState<Record<string,string>>({});
 const [targetTask,setTargetTask]=useState("");const [supplement,setSupplement]=useState("");const [manualReport,setManualReport]=useState("");const [inspection,setInspection]=useState<TaskInspection|null>(null);
 const [concurrency,setConcurrency]=useState(1);const [budget,setBudget]=useState("");const [unknownCost,setUnknownCost]=useState(true);const [repair,setRepair]=useState("bounded-ladder");
 const [draft,setDraft]=useState<Plan|null>(null);
 const s=projection?.snapshot;const plan=s?.plan?.plan;
 useEffect(()=>{setDraft(null);},[plan?.rawMarkdown]);
 useEffect(()=>observeRuntime(sessionId),[sessionId]);
 useEffect(()=>{setText(plan?.rawMarkdown??"");setConfirmed([]);setInspection(null);},[plan?.rawMarkdown,s?.plan?.confirmationDigest]);
 useEffect(()=>{if(s){setConcurrency(s.config.concurrency);setBudget(s.config.budgetUsd==null?"":String(s.config.budgetUsd));setUnknownCost(s.config.blockUnknownCost);setRepair(s.config.repairPolicy);}},[s?.configHash]);
 const intent=async(type:string,payload?:Record<string,unknown>)=>{if(!["CancelRun","ResumeRun"].includes(type)&&useProjects.getState().projects.find(p=>p.sessions.some(x=>x.id===sessionId))?.trusted!==true){setError(t("settings.cli.trust.blocked"));return;}setBusy(true);setError("");try{await useRuntime.getState().command(sessionId,{type,payload});}catch(e){setError(String(e));}finally{setBusy(false);}};
 if(!s)return <p className="p-5 text-muted">{observationError||t("runtime.loading")}</p>;
 const commands=[...new Set(plan?.tasks.flatMap(task=>task.acceptance.commands)??[])];
 const approval=s.plan?.confirmationDigest;
 const amendable=!!s.execution&&["Blocked","Stopped","AwaitingApproval","Approved"].includes(s.state);
 const inspect=async(taskId=targetTask)=>{setTargetTask(taskId);setError("");try{setInspection(await runtimeInspectTask(sessionId,taskId));}catch(e){setError(String(e));}};
 const amend=async()=>{if(!window.confirm(t("runtime.confirmAmend")))return;const config={...s.config,concurrency,budgetUsd:budget.trim()?Number(budget):null,blockUnknownCost:unknownCost,repairPolicy:repair};await intent(amendable?"AmendRun":"UpdateConfig",amendable?{confirmationDigest:approval,text,config}:{config});};
 const confirm=async()=>{
  if(!window.confirm(t("runtime.confirmPlan")))return;
  // The normal per-session Full Auto grant remains explicit and expires with the application.
  useAuth.getState().grantFullAuto(sessionId,"session");
  await intent("ConfirmPlan",{confirmationDigest:approval,confirmedCommands:confirmed});
 };
 const start=async()=>{
  if(!useAuth.getState().isFullAutoActive(sessionId)){
    if(!window.confirm(t("runtime.confirmPlan")))return;
    useAuth.getState().grantFullAuto(sessionId,"session");
  }
  await intent("StartExecution",{confirmationDigest:s.approvedDigest});
 };
 const deliver=async(type:string)=>{
  if(!window.confirm(t("runtime.confirmDelivery",{source:s.delivery?.candidate?.head,target:s.config.targetIdentity})))return;
  await intent(type,{confirmationDigest:s.delivery?.confirmationDigest,...(type==="ExportPatch"?{outputPath:output}:{})});
 };
 const editable=!s.execution&&["Draft","AwaitingApproval","Approved","Blocked"].includes(s.state);
 const active=["Planning","Executing","Reviewing","AwaitingReview","AwaitingTurnApproval","AwaitingMerge"].includes(s.state);
 const durableError=s.delivery?.intent?.error??Object.values(s.execution?.tasks??{}).find(task=>task.error)?.error??Object.values(s.turns??{}).find(turn=>turn.error)?.error;
 const modelControls=(taskId:string)=><div className="mt-3">{(editable||amendable)&&s.execution?.tasks[taskId]?.state!=="DONE"&&!s.config.models.executor?.userLocked&&<div className="space-x-2"><select aria-label={t("runtime.taskModel",{task:taskId})} value={modelPicks[taskId]??""} onChange={e=>setModelPicks(old=>({...old,[taskId]:e.target.value}))} className="h-8 max-w-full rounded-md border border-border bg-elevated px-2 text-xs"><option value="">{t("runtime.chooseModel")}</option>{Object.entries(s.config.models).map(([key,model])=><option key={key} value={key}>{model.provider} · {model.requestedModel}{model.modelParams?.effort?` · ${model.modelParams.effort}`:""}</option>)}</select><Button disabled={busy||!!draft||!modelPicks[taskId]} onClick={()=>{const selected=s.config.models[modelPicks[taskId]];if(!selected)return;const config={...s.config,models:{...s.config.models,[taskId]:{...selected,userLocked:true}}};if(window.confirm(t("runtime.confirmAmend")))void intent(amendable?"AmendRun":"UpdateConfig",amendable?{confirmationDigest:approval,text,config}:{config});}}>{t("runtime.applyModel")}</Button></div>}</div>;
 const saveStructured=async()=>{
  if(!draft)return;
  setBusy(true);setError("");
  try{
   let markdown=await invoke<string>("runtime_render_plan",{plan:draft});
   // The native renderer serializes structured fields. Preserve an explicitly edited raw Spec
   // section and let the normal runtime ImportPlan/AmendRun parser validate the full document.
   if(plan && draft.spec.raw!==plan.spec.raw){
    const nextSection=markdown.search(/\n# (Contracts|Tasks)\n/);
    if(nextSection<0)throw new Error("Rendered plan has no task section");
    const spec=draft.spec.raw.trim();
    markdown=(/^# Spec(?:\s|$)/.test(spec)?spec:`# Spec\n\n${spec}`)+"\n"+markdown.slice(nextSection);
   }
   if(amendable){if(!window.confirm(t("runtime.confirmAmend")))return;await intent("AmendRun",{confirmationDigest:approval,text:markdown,config:s.config});}
   else await intent("ImportPlan",{text:markdown});
  }catch(e){setError(String(e));}finally{setBusy(false);}
 };
 if(view==="session")return <RuntimeHistoryView sessionId={sessionId}/>;
 const approvalControls=<>{Object.values(s.turns??{}).filter(turn=>turn.approval&&!turn.approval.decision).map(turn=><div key={turn.id} className="rounded-xl border border-warning/30 bg-warning/5 p-4 space-y-3"><code className="block whitespace-pre-wrap break-words text-xs">{turn.approval?.command}</code><div className="flex flex-wrap gap-2">{(["AllowOnce","AllowSession","Deny"] as const).map(decision=><Button key={decision} disabled={busy} onClick={()=>{if(window.confirm(t("runtime.confirmApproval",{command:turn.approval?.command})))void intent("ResolveApproval",{decision,approvalId:turn.approval?.id,turnId:turn.id,confirmationDigest:turn.approval?.confirmationDigest});}}>{t(`runtime.decisions.${decision}`)}</Button>)}</div></div>)}</>;
 const runControls=<div className="space-y-3"><div className="flex gap-2 flex-wrap">
   {editable&&!plan&&<Button disabled={busy} onClick={()=>void intent("GeneratePlan")}>{t("runtime.generate")}</Button>}
   {s.state==="AwaitingApproval"&&<Button disabled={busy||!!draft||commands.some(command=>!confirmed.includes(command))} onClick={()=>void confirm()}>{t("runtime.approve")}</Button>}
   {s.state==="Approved"&&<Button disabled={busy||!!draft} onClick={()=>void start()}>{t("runtime.execute")}</Button>}
   {active&&<Button disabled={busy} onClick={()=>void intent("CancelRun")}>{t("runtime.cancel")}</Button>}
   {["Blocked","Stopped"].includes(s.state)&&<Button disabled={busy} onClick={()=>{if(window.confirm(t("runtime.confirmRetry")))void intent("PrepareRetry",{confirmationDigest:approval});}}>{t("runtime.retry")}</Button>}
   {["Planning","Executing","Reviewing","Delivering","Blocked","Stopping","CancelRequested"].includes(s.state)&&<Button disabled={busy} onClick={()=>void intent("ResumeRun")}>{t("runtime.reconcile")}</Button>}
   {s.state==="Blocked"&&<Button disabled={busy} onClick={()=>void intent("ReviewDelivery")}>{t("runtime.review")}</Button>}
  </div>{s.state==="AwaitingApproval"&&<fieldset className="text-sm space-y-2"><legend>{t("runtime.commands")}</legend>{commands.map(command=><label className="flex gap-2" key={command}><input type="checkbox" checked={confirmed.includes(command)} onChange={e=>setConfirmed(old=>e.target.checked?[...old,command]:old.filter(c=>c!==command))}/><code>{command}</code></label>)}</fieldset>}</div>;
 if(view==="plan"&&plan)return <div className="flex min-h-0 flex-1 flex-col"><div className="shrink-0 space-y-2 border-b border-border px-5 py-3 text-xs">{(error||observationError||durableError)&&<p role="alert" className="text-danger">{error||observationError||durableError}</p>}{runControls}</div><RuntimePlanReview plan={draft??plan} assignments={s.plan?.assignments} editable={editable||amendable} dirty={!!draft} busy={busy} onChange={setDraft} onSave={()=>void saveStructured()} taskModels={<div className="space-y-3 p-5">{plan.tasks.map(task=><div key={task.id} className="rounded-lg border border-border p-3 text-xs"><strong>{task.id} · {s.plan?.assignments[task.id]?.requestedModel}</strong>{modelControls(task.id)}</div>)}</div>}/></div>;
 if(view==="chat")return <RuntimeConversation sessionId={sessionId}>
  <div className="rounded-xl border border-border bg-elevated p-4 text-sm space-y-3">
   <p className="font-medium">{t("runtime.title")} · {t(`runtime.states.${s.state}`,{defaultValue:s.state})}</p>
   {(error||observationError||durableError)&&<p role="alert" className="text-danger">{error||observationError||durableError}</p>}
   <div className="flex flex-wrap gap-2"><Button size="sm" onClick={()=>onSwitchTab?.("plan")}>{t("workspace.tabs.plan")}</Button><Button size="sm" onClick={()=>onSwitchTab?.("pipeline")}>{t("workspace.tabs.pipeline")}</Button>{active&&<Button size="sm" variant="ghost" disabled={busy} onClick={()=>void intent("CancelRun")}>{t("runtime.cancel")}</Button>}</div>
   {approvalControls}
  </div>
 </RuntimeConversation>;
 return <section className="flex min-h-0 flex-1 flex-col overflow-hidden" aria-label={t("runtime.title")}><div className="flex min-h-0 flex-1 flex-col gap-3 px-4 py-4">
  <header className="space-y-2"><h2 className="text-lg font-semibold">{t("runtime.title")}</h2><p className="text-sm text-muted">{t(`runtime.states.${s.state}`,{defaultValue:s.state})}</p><details className="text-xs text-muted"><summary className="cursor-pointer">{t("runtime.requirement")}</summary><p className="mt-2 whitespace-pre-wrap wrap-anywhere">{s.requirement}</p><p className="mt-2 break-all">{s.config.workspace}</p></details></header>
  {(error||observationError||durableError)&&<p role="alert" className="text-sm text-danger">{error||observationError||durableError}</p>}
  <p className="text-xs text-muted">{t("runtime.config",{concurrency:s.config.concurrency,budget:s.config.budgetUsd??t("runtime.unlimited"),repair:s.config.repairPolicy})}</p>
  <div className="flex gap-2 flex-wrap">
   {editable&&!plan&&<Button disabled={busy} onClick={()=>void intent("GeneratePlan")}>{t("runtime.generate")}</Button>}
   {s.state==="AwaitingApproval"&&<Button disabled={busy||!!draft||commands.some(command=>!confirmed.includes(command))} onClick={()=>void confirm()}>{t("runtime.approve")}</Button>}
   {s.state==="Approved"&&<Button disabled={busy||!!draft} onClick={()=>void start()}>{t("runtime.execute")}</Button>}
   {active&&<Button disabled={busy} onClick={()=>void intent("CancelRun")}>{t("runtime.cancel")}</Button>}
   {["Blocked","Stopped"].includes(s.state)&&<Button disabled={busy} onClick={()=>{if(window.confirm(t("runtime.confirmRetry")))void intent("PrepareRetry",{confirmationDigest:approval});}}>{t("runtime.retry")}</Button>}
   {["Planning","Executing","Reviewing","Delivering","Blocked","Stopping","CancelRequested"].includes(s.state)&&<Button disabled={busy} onClick={()=>void intent("ResumeRun")}>{t("runtime.reconcile")}</Button>}
   {s.state==="Blocked"&&<Button disabled={busy} onClick={()=>void intent("ReviewDelivery")}>{t("runtime.review")}</Button>}
  </div>
  {(amendable||editable)&&<details open={!!inspection} className="max-h-[35vh] shrink-0 overflow-y-auto rounded-xl border border-border p-3"><summary className="cursor-pointer text-sm text-muted">{t("runtime.amend")}</summary><fieldset className="rounded border border-border p-3 text-sm space-y-3"><legend>{t("runtime.amend")}</legend>
   <label className="block">{t("runtime.concurrency")}<input type="number" min={1} className="ml-2 w-20 bg-transparent border border-border" value={concurrency} onChange={e=>setConcurrency(Number(e.target.value))}/></label>
   <label className="block">{t("runtime.budget")}<input type="number" min={0} className="ml-2 w-28 bg-transparent border border-border" value={budget} onChange={e=>setBudget(e.target.value)}/></label>
   <label className="flex gap-2"><input type="checkbox" checked={unknownCost} onChange={e=>setUnknownCost(e.target.checked)}/>{t("runtime.unknownCost")}</label>
   <label className="block">{t("runtime.repairPolicy")}<select className="ml-2 bg-transparent" value={repair} onChange={e=>setRepair(e.target.value)}><option value="manual">{t("runtime.manualRepair")}</option><option value="bounded-ladder">{t("runtime.boundedRepair")}</option></select></label>
   <Button disabled={busy} onClick={()=>void amend()}>{t("runtime.applyAmend")}</Button>
   <label className="block">{t("runtime.task")}<select className="ml-2 bg-transparent" value={targetTask} onChange={e=>{setTargetTask(e.target.value);setInspection(null);}}><option value="">{t("runtime.selectTask")}</option>{plan?.tasks.filter(task=>s.execution?.tasks[task.id]?.state!=="DONE").map(task=><option key={task.id} value={task.id}>{task.id} · {task.title}</option>)}</select></label>
   <textarea aria-label={t("runtime.supplement")} className="w-full bg-transparent border border-border p-2" value={supplement} onChange={e=>setSupplement(e.target.value)}/>
   <Button disabled={busy||!targetTask||!supplement.trim()} onClick={()=>{if(window.confirm(t("runtime.confirmAmend")))void intent("SupplementTask",{confirmationDigest:approval,taskId:targetTask,text:supplement});}}>{t("runtime.supplement")}</Button>
   <Button disabled={busy||!targetTask} onClick={()=>void inspect()}>{t("runtime.inspectTakeover")}</Button>
   {inspection&&<div className="space-y-2"><p className="break-all text-xs">{inspection.workspace.cwd}</p><Button onClick={()=>void openPathIn(inspection.workspace.cwd,"TERMINAL").catch(e=>setError(String(e)))}>{t("runtime.openTerminal")}</Button><p>{t("runtime.humanReport")}</p><p className="text-xs text-muted">{t("runtime.reportFormat")}</p><textarea aria-label={t("runtime.humanReport")} className="w-full min-h-28 bg-transparent border border-border p-2" value={manualReport} onChange={e=>setManualReport(e.target.value)}/><Button disabled={busy||!manualReport.trim()} onClick={()=>{if(window.confirm(t("runtime.confirmTakeover")))void intent("AcceptTakeover",{taskId:targetTask,sourceDigest:inspection.sourceDigest,confirmationDigest:inspection.confirmationDigest,report:manualReport});}}>{t("runtime.acceptTakeover")}</Button></div>}
  </fieldset></details>}
  {s.state==="AwaitingApproval"&&<fieldset className="text-sm space-y-2"><legend>{t("runtime.commands")}</legend>{commands.map(command=><label className="flex gap-2" key={command}><input type="checkbox" checked={confirmed.includes(command)} onChange={e=>setConfirmed(old=>e.target.checked?[...old,command]:old.filter(c=>c!==command))}/><code>{command}</code></label>)}</fieldset>}
  {plan&&<div className="flex min-h-48 flex-1 flex-col"><RuntimeTaskBoard snapshot={s} busy={busy} modelControls={modelControls} onRetry={taskId=>{if(window.confirm(t("runtime.confirmRetry")))void intent("RetryTask",{taskId});}} onInspect={taskId=>void inspect(taskId)} /></div>}
  {approvalControls}
  {s.state==="AwaitingMerge"&&<div className="space-y-3"><p className="text-xs break-all">{s.delivery?.candidate?.head} → {s.config.targetIdentity}</p><Button disabled={busy} onClick={()=>void deliver("MergeDelivery")}>{t("runtime.merge")}</Button><Button disabled={busy} onClick={()=>void deliver("KeepBranch")}>{t("runtime.keepBranch")}</Button><label className="block text-sm">{t("runtime.output")}<input className="ml-2 border border-border bg-transparent" value={output} onChange={e=>setOutput(e.target.value)}/></label><Button disabled={busy||!output.trim()} onClick={()=>void deliver("ExportPatch")}>{t("runtime.export")}</Button></div>}
  {s.delivery?.artifact&&<p className="text-sm break-all">{s.delivery.artifact}</p>}
 </div></section>;
}
