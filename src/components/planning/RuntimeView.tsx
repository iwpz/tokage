import { SlidersHorizontal, Info, ShieldCheck, Play, Square, RotateCcw, RefreshCw, SearchCheck, GitMerge, GitBranch, Download, Save, FilePlus, Terminal } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import type { Plan } from "../../lib/ipc";
import { runtimeFailure } from "../../lib/runtimeFailure";
import { RuntimeActionPanel } from "./RuntimeActionPanel";
import { RuntimePlanReview } from "./RuntimePlanReview";
import { RuntimeDeliveryEvidence } from "./RuntimeDeliveryEvidence";
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
import { Button, Checkbox, Modal, Switch } from "../ui";
export function RuntimeView({sessionId,view="pipeline",onSwitchTab}:{sessionId:string;view?:"chat"|"plan"|"pipeline"|"session";onSwitchTab?:(tab:WorkspaceTab)=>void}){
 const {t}=useTranslation();const projection=useRuntime(s=>s.runs[sessionId]);const observationError=useRuntime(s=>s.errors[sessionId]);
 const [error,setError]=useState("");const [busy,setBusy]=useState(false);const [text,setText]=useState("");const [confirmed,setConfirmed]=useState<string[]>([]);const [output,setOutput]=useState("");
 const [modelPicks,setModelPicks]=useState<Record<string,string>>({});
 const [targetTask,setTargetTask]=useState("");const [supplement,setSupplement]=useState("");const [manualReport,setManualReport]=useState("");const [inspection,setInspection]=useState<TaskInspection|null>(null);
 const [concurrency,setConcurrency]=useState(1);const [budget,setBudget]=useState("");const [unknownCost,setUnknownCost]=useState(true);const [repair,setRepair]=useState("bounded-ladder");
 const [approvalOpen,setApprovalOpen]=useState(false);const [policyOpen,setPolicyOpen]=useState(false);const [requirementOpen,setRequirementOpen]=useState(false);
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
 const inspect=async(taskId=targetTask)=>{setPolicyOpen(true);setTargetTask(taskId);setError("");try{setInspection(await runtimeInspectTask(sessionId,taskId));}catch(e){setError(String(e));}};
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
 const failure=runtimeFailure(s);
 const durableError=failure.cleanupUnknown?t("runtime.blockedCleanupUnknown",{models:failure.cleanupModels}):failure.unknownCost?t("runtime.blockedUnknownCost",{models:failure.models}):failure.message??(s.state==="Blocked"?t("runtime.blockedMissingReason"):undefined);
 const failureDetails=s.state==="Blocked"&&<div role="alert" className="mx-4 mt-3 shrink-0 rounded-xl border border-warning/30 bg-warning/5 px-4 py-3 text-xs"><p className="font-semibold text-warning">{t("runtime.blockedTitle")}</p><p className="mt-1 break-words leading-relaxed">{durableError}</p>{failure.cleanupUnknown&&<p className="mt-2 leading-relaxed">{t("runtime.blockedCleanupUnknownHelp")}</p>}{(failure.unknownCost||failure.cleanupUnknown)&&<details className="mt-2 text-muted"><summary className="cursor-pointer">{t("runtime.technicalDetails")}</summary>{failure.unknownCost&&<p className="mt-1 leading-relaxed">{t("runtime.blockedUnknownCostHelp")}</p>}{failure.message&&<pre className="mt-1 whitespace-pre-wrap break-words">{failure.message}</pre>}</details>}</div>;

 const modelControls=(taskId:string)=><div className="mt-3">{(editable||amendable)&&s.execution?.tasks[taskId]?.state!=="DONE"&&!s.config.models.executor?.userLocked&&<div className="flex min-w-0 flex-wrap items-center gap-2"><select aria-label={t("runtime.taskModel",{task:taskId})} value={modelPicks[taskId]??""} onChange={e=>setModelPicks(old=>({...old,[taskId]:e.target.value}))} className="h-8 max-w-full rounded-md border border-border bg-elevated px-2 text-xs"><option value="">{t("runtime.chooseModel")}</option>{Object.entries(s.config.models).map(([key,model])=><option key={key} value={key}>{model.provider} · {model.requestedModel}{model.modelParams?.effort?` · ${model.modelParams.effort}`:""}</option>)}</select><Button size="sm" disabled={busy||!!draft||!modelPicks[taskId]} onClick={()=>{const selected=s.config.models[modelPicks[taskId]];if(!selected)return;const config={...s.config,models:{...s.config.models,[taskId]:{...selected,userLocked:true}}};if(window.confirm(t("runtime.confirmAmend")))void intent(amendable?"AmendRun":"UpdateConfig",amendable?{confirmationDigest:approval,text,config}:{config});}}>{t("runtime.applyModel")}</Button></div>}</div>;
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
 const approvalControls=<>{Object.values(s.turns??{}).filter(turn=>turn.approval&&!turn.approval.decision).map(turn=><div key={turn.id} className="rounded-xl border border-warning/30 bg-warning/5 p-4 space-y-3"><code className="block whitespace-pre-wrap break-words text-xs">{turn.approval?.command}</code><div className="flex flex-wrap gap-2">{(["AllowOnce","AllowSession","Deny"] as const).map(decision=><Button size="sm" key={decision} disabled={busy} onClick={()=>{if(window.confirm(t("runtime.confirmApproval",{command:turn.approval?.command})))void intent("ResolveApproval",{decision,approvalId:turn.approval?.id,turnId:turn.id,confirmationDigest:turn.approval?.confirmationDigest});}}>{t(`runtime.decisions.${decision}`)}</Button>)}</div></div>)}</>;
 const dialogs=<>
  <Modal open={approvalOpen} onClose={()=>{if(!busy)setApprovalOpen(false);}} title={t("runtime.openApproval")}><div className="space-y-4">{s.state==="AwaitingApproval"&&<fieldset className="text-sm space-y-2"><legend>{t("runtime.commands")}</legend>{commands.map(command=><label className="flex cursor-pointer items-start gap-2.5 rounded-lg border border-border bg-base px-3 py-2.5" key={command}><Checkbox disabled={busy} checked={confirmed.includes(command)} onChange={e=>setConfirmed(old=>e.target.checked?[...old,command]:old.filter(c=>c!==command))}/><code className="min-w-0 break-all text-xs leading-4">{command}</code></label>)}</fieldset>}<p className="text-xs text-muted">{t("runtime.confirmPlan")}</p><div className="flex justify-end"><Button size="sm" variant="primary" disabled={busy||!!draft||commands.some(command=>!confirmed.includes(command))} onClick={()=>void confirm().then(()=>setApprovalOpen(false))}>{t("runtime.approve")}</Button></div></div></Modal>
  <Modal open={policyOpen} onClose={()=>{if(!busy)setPolicyOpen(false);}} title={t("runtime.amend")}><fieldset disabled={busy} className="min-w-0 space-y-4 border-0 p-0 text-sm">
   <label className="block">{t("runtime.concurrency")}<input type="number" min={1} className="ml-2 h-8 w-24 rounded-md border border-border bg-base px-2 text-xs" value={concurrency} onChange={e=>setConcurrency(Number(e.target.value))}/></label>
   <label className="block">{t("runtime.budget")}<input type="number" min={0} className="ml-2 h-8 w-28 rounded-md border border-border bg-base px-2 text-xs" value={budget} onChange={e=>setBudget(e.target.value)}/></label>
   <div className="flex items-center justify-between gap-4"><span className="text-xs text-fg">{t("runtime.unknownCost")}</span><Switch disabled={busy} label={t("runtime.unknownCost")} checked={unknownCost} onChange={setUnknownCost}/></div>
   <label className="block">{t("runtime.repairPolicy")}<select className="ml-2 h-8 max-w-full rounded-md border border-border bg-base px-2 text-xs" value={repair} onChange={e=>setRepair(e.target.value)}><option value="manual">{t("runtime.manualRepair")}</option><option value="bounded-ladder">{t("runtime.boundedRepair")}</option></select></label>
   <Button size="sm" disabled={busy} onClick={()=>void amend()}><Save className="size-3.5"/>{t("runtime.applyAmend")}</Button>
   <label className="block">{t("runtime.task")}<select className="ml-2 h-8 max-w-full rounded-md border border-border bg-base px-2 text-xs" value={targetTask} onChange={e=>{setTargetTask(e.target.value);setInspection(null);}}><option value="">{t("runtime.selectTask")}</option>{plan?.tasks.filter(task=>s.execution?.tasks[task.id]?.state!=="DONE").map(task=><option key={task.id} value={task.id}>{task.id} · {task.title}</option>)}</select></label>
   <textarea aria-label={t("runtime.supplement")} className="w-full bg-transparent border border-border p-2" value={supplement} onChange={e=>setSupplement(e.target.value)}/>
   <Button size="sm" disabled={busy||!targetTask||!supplement.trim()} onClick={()=>{if(window.confirm(t("runtime.confirmAmend")))void intent("SupplementTask",{confirmationDigest:approval,taskId:targetTask,text:supplement});}}>{t("runtime.supplement")}</Button>
   <Button size="sm" disabled={busy||!targetTask} onClick={()=>void inspect()}>{t("runtime.inspectTakeover")}</Button>
   {inspection&&<div className="space-y-2"><p className="break-all text-xs">{inspection.workspace.cwd}</p><Button size="sm" onClick={()=>void openPathIn(inspection.workspace.cwd,"TERMINAL").catch(e=>setError(String(e)))}><Terminal className="size-3.5"/>{t("runtime.openTerminal")}</Button><p>{t("runtime.humanReport")}</p><p className="text-xs text-muted">{t("runtime.reportFormat")}</p><textarea aria-label={t("runtime.humanReport")} className="w-full min-h-28 bg-transparent border border-border p-2" value={manualReport} onChange={e=>setManualReport(e.target.value)}/><Button size="sm" disabled={busy||!manualReport.trim()} onClick={()=>{if(window.confirm(t("runtime.confirmTakeover")))void intent("AcceptTakeover",{taskId:targetTask,sourceDigest:inspection.sourceDigest,confirmationDigest:inspection.confirmationDigest,report:manualReport});}}>{t("runtime.acceptTakeover")}</Button></div>}
  </fieldset></Modal>
  <Modal open={requirementOpen} onClose={()=>setRequirementOpen(false)} title={t("runtime.requirement")}><p className="whitespace-pre-wrap break-words text-sm">{s.requirement}</p><p className="mt-3 break-all text-xs text-muted">{s.config.workspace}</p></Modal>
 </>;
 const runControls=<div className="flex min-w-0 flex-wrap items-center gap-2">
   {editable&&!plan&&<Button size="sm" disabled={busy} onClick={()=>void intent("GeneratePlan")}><FilePlus className="size-3.5"/>{t("runtime.generate")}</Button>}
   {s.state==="AwaitingApproval"&&<Button size="sm" variant="primary" disabled={busy||!!draft} onClick={()=>setApprovalOpen(true)}><ShieldCheck className="size-3.5"/>{t("runtime.openApproval")}</Button>}
   {s.state==="Approved"&&<Button size="sm" variant="primary" disabled={busy||!!draft} onClick={()=>void start()}><Play className="size-3.5"/>{t("runtime.execute")}</Button>}
   {active&&<Button size="sm" disabled={busy} onClick={()=>void intent("CancelRun")}><Square className="size-3.5"/>{t("runtime.cancel")}</Button>}
   {approval&&["Blocked","Stopped"].includes(s.state)&&<Button size="sm" disabled={busy} onClick={()=>{if(window.confirm(t("runtime.confirmRetry")))void intent("PrepareRetry",{confirmationDigest:approval});}}><RotateCcw className="size-3.5"/>{t("runtime.retry")}</Button>}
   {["Planning","Executing","Reviewing","Delivering","Blocked","Stopping","CancelRequested"].includes(s.state)&&<Button size="sm" disabled={busy} onClick={()=>void intent("ResumeRun")}><RefreshCw className="size-3.5"/>{t("runtime.reconcile")}</Button>}
   {s.state==="Blocked"&&<Button size="sm" disabled={busy} onClick={()=>void intent("ReviewDelivery")}><SearchCheck className="size-3.5"/>{t("runtime.review")}</Button>}
  {(editable||amendable)&&<Button size="sm" variant="ghost" aria-label={t("runtime.settings")} title={t("runtime.settings")} onClick={()=>setPolicyOpen(true)}><SlidersHorizontal className="size-3.5"/></Button>}<Button size="sm" variant="ghost" aria-label={t("runtime.requirement")} onClick={()=>setRequirementOpen(true)}><Info className="size-3.5"/></Button></div>;
 const actionPanel=<RuntimeActionPanel snapshot={s} error={error||observationError||(s.state!=="Blocked"?durableError:undefined)}>{runControls}{s.state==="AwaitingMerge"&&<div className="flex min-w-0 flex-wrap items-center gap-2"><p className="max-w-64 break-all font-mono text-[10px] text-subtle">{s.delivery?.candidate?.head} → {s.config.targetIdentity}</p><Button size="sm" variant="primary" disabled={busy} onClick={()=>void deliver("MergeDelivery")}><GitMerge className="size-3.5"/>{t("runtime.merge")}</Button><Button size="sm" disabled={busy} onClick={()=>void deliver("KeepBranch")}><GitBranch className="size-3.5"/>{t("runtime.keepBranch")}</Button><label className="flex min-w-0 flex-wrap items-center gap-2 text-xs">{t("runtime.output")}<input className="h-7 min-w-0 max-w-44 rounded-md border border-border bg-base px-2 text-xs" value={output} onChange={e=>setOutput(e.target.value)}/></label><Button size="sm" disabled={busy||!output.trim()} onClick={()=>void deliver("ExportPatch")}><Download className="size-3.5"/>{t("runtime.export")}</Button></div>}
  {s.delivery?.artifact&&<p className="text-sm break-all">{s.delivery.artifact}</p>}</RuntimeActionPanel>;
 if(view==="plan"&&plan)return <div className="flex min-h-0 min-w-0 flex-1 flex-col">{failureDetails}<RuntimePlanReview plan={draft??plan} assignments={s.plan?.assignments} editable={editable||amendable} dirty={!!draft} busy={busy} onChange={setDraft} onSave={()=>void saveStructured()} taskModels={<div className="space-y-3 p-5">{plan.tasks.map(task=><div key={task.id} className="rounded-lg border border-border p-3 text-xs"><strong>{task.id} · {s.plan?.assignments[task.id]?.requestedModel}</strong>{modelControls(task.id)}</div>)}</div>}/>{actionPanel}{dialogs}</div>;
 if(view==="chat")return <RuntimeConversation sessionId={sessionId} onSwitchTab={onSwitchTab}>
  {(error||observationError||durableError)&&<p role="alert" className="text-xs text-danger">{error||observationError||durableError}</p>}
  {approvalControls}
 </RuntimeConversation>;
 return <section className="flex min-h-0 flex-1 flex-col overflow-hidden" aria-label={t("runtime.title")}><div className="flex min-h-0 flex-1 flex-col gap-3">

  {failureDetails}
  {plan&&<div className="flex min-h-0 flex-1 flex-col"><RuntimeTaskBoard snapshot={s} onSwitchTab={onSwitchTab} busy={busy} modelControls={modelControls} onRetry={taskId=>{if(window.confirm(t("runtime.confirmRetry")))void intent("RetryTask",{taskId});}} onInspect={taskId=>void inspect(taskId)} /></div>}
  <RuntimeDeliveryEvidence snapshot={s}/>
  {approvalControls}

 {actionPanel}{dialogs}</div></section>;
}
