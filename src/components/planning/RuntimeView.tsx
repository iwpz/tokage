import { useEffect,useState } from "react";
import { useTranslation } from "react-i18next";
import { observeRuntime,useRuntime } from "../../stores/runtime";
import { useAuth } from "../../stores/auth";
import { Button } from "../ui";
export function RuntimeView({sessionId}:{sessionId:string}){
 const {t}=useTranslation();const projection=useRuntime(s=>s.runs[sessionId]);const observationError=useRuntime(s=>s.errors[sessionId]);
 const [error,setError]=useState("");const [busy,setBusy]=useState(false);const [text,setText]=useState("");const [confirmed,setConfirmed]=useState<string[]>([]);const [output,setOutput]=useState("");
 const s=projection?.snapshot;const plan=s?.plan?.plan;
 useEffect(()=>observeRuntime(sessionId),[sessionId]);
 useEffect(()=>{setText(plan?.rawMarkdown??"");setConfirmed([]);},[plan?.rawMarkdown,s?.plan?.confirmationDigest]);
 const intent=async(type:string,payload?:Record<string,unknown>)=>{setBusy(true);setError("");try{await useRuntime.getState().command(sessionId,{type,payload});}catch(e){setError(String(e));}finally{setBusy(false);}};
 if(!s)return <p className="p-5 text-muted">{observationError||t("runtime.loading")}</p>;
 const commands=[...new Set(plan?.tasks.flatMap(task=>task.acceptance.commands)??[])];
 const approval=s.plan?.confirmationDigest;
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
 const editable=["Draft","AwaitingApproval","Approved"].includes(s.state);
 const active=["Planning","Executing","Reviewing","AwaitingReview","AwaitingTurnApproval","AwaitingMerge"].includes(s.state);
 const durableError=s.delivery?.intent?.error??Object.values(s.execution?.tasks??{}).find(task=>task.error)?.error??Object.values(s.turns??{}).find(turn=>turn.error)?.error;
 return <section className="min-h-0 flex-1 overflow-auto p-5 space-y-4" aria-label={t("runtime.title")}>
  <header><h2 className="font-semibold">{t("runtime.title")}</h2><p className="text-sm text-muted">{t(`runtime.states.${s.state}`,{defaultValue:s.state})}</p><p className="text-xs text-muted break-all">{s.requirement}</p></header>
  {(error||observationError||durableError)&&<p role="alert" className="text-sm text-danger">{error||observationError||durableError}</p>}
  <p className="text-xs text-muted">{t("runtime.config",{concurrency:s.config.concurrency,budget:s.config.budgetUsd??t("runtime.unlimited"),repair:s.config.repairPolicy})}</p>
  <div className="flex gap-2 flex-wrap">
   {editable&&<Button disabled={busy} onClick={()=>void intent("GeneratePlan")}>{t("runtime.generate")}</Button>}
   {s.state==="AwaitingApproval"&&<Button disabled={busy} onClick={()=>void confirm()}>{t("runtime.approve")}</Button>}
   {s.state==="Approved"&&<Button disabled={busy} onClick={()=>void start()}>{t("runtime.execute")}</Button>}
   {active&&<Button disabled={busy} onClick={()=>void intent("CancelRun")}>{t("runtime.cancel")}</Button>}
   {["Blocked","Stopped"].includes(s.state)&&<Button disabled={busy} onClick={()=>{if(window.confirm(t("runtime.confirmRetry")))void intent("PrepareRetry",{confirmationDigest:approval});}}>{t("runtime.retry")}</Button>}
   {["Planning","Executing","Reviewing","Delivering","Blocked","Stopping","CancelRequested"].includes(s.state)&&<Button disabled={busy} onClick={()=>void intent("ResumeRun")}>{t("runtime.reconcile")}</Button>}
   {s.state==="Blocked"&&<Button disabled={busy} onClick={()=>void intent("ReviewDelivery")}>{t("runtime.review")}</Button>}
  </div>
  {editable&&<div className="space-y-2"><label className="block text-sm" htmlFor={`plan-${sessionId}`}>{t("runtime.plan")}</label><textarea id={`plan-${sessionId}`} className="min-h-56 w-full rounded border border-border bg-transparent p-3 font-mono text-xs" value={text} onChange={e=>setText(e.target.value)} /><Button disabled={busy||!text.trim()} onClick={()=>void intent("ImportPlan",{text})}>{t("runtime.savePlan")}</Button></div>}
  {s.state==="AwaitingApproval"&&<fieldset className="text-sm space-y-2"><legend>{t("runtime.commands")}</legend>{commands.map(command=><label className="flex gap-2" key={command}><input type="checkbox" checked={confirmed.includes(command)} onChange={e=>setConfirmed(old=>e.target.checked?[...old,command]:old.filter(c=>c!==command))}/><code>{command}</code></label>)}</fieldset>}
  {plan?.tasks.map(task=>{const state=s.execution?.tasks[task.id];const assignment=s.plan?.assignments[task.id];return <article key={task.id} className="rounded border border-border p-3 text-sm"><strong>{task.id} · {task.title}</strong><p>{state?.state??t("runtime.pending")}</p><p className="text-xs text-muted">{assignment?.provider} · {assignment?.requestedModel}</p><p className="text-xs">{task.write_scope.join(", ")}</p>{state?.error&&<p role="alert">{state.error}</p>}</article>;})}
  {Object.values(s.turns??{}).map(turn=><details key={turn.id} open={!turn.ended}><summary className="text-sm">{turn.role} · {turn.requested.requestedModel} · {turn.observedModel??t("runtime.unknownModel")} · {turn.status}</summary><pre className="whitespace-pre-wrap break-words text-xs">{projection.transcripts[turn.id]?.map(row=>["text","reasoning"].includes(row.event.kind)&&"text"in row.event?row.event.text:JSON.stringify(row.event)).join("\n")}</pre>{turn.approval&&!turn.approval.decision&&<div className="space-x-2"><code>{turn.approval.command}</code>{(["AllowOnce","AllowSession","Deny"] as const).map(decision=><Button key={decision} disabled={busy} onClick={()=>{if(window.confirm(t("runtime.confirmApproval",{command:turn.approval?.command})))void intent("ResolveApproval",{decision,approvalId:turn.approval?.id,turnId:turn.id,confirmationDigest:turn.approval?.confirmationDigest});}}>{t(`runtime.decisions.${decision}`)}</Button>)}</div>}</details>)}
  {s.state==="AwaitingMerge"&&<div className="space-y-3"><p className="text-xs break-all">{s.delivery?.candidate?.head} → {s.config.targetIdentity}</p><Button disabled={busy} onClick={()=>void deliver("MergeDelivery")}>{t("runtime.merge")}</Button><Button disabled={busy} onClick={()=>void deliver("KeepBranch")}>{t("runtime.keepBranch")}</Button><label className="block text-sm">{t("runtime.output")}<input className="ml-2 border border-border bg-transparent" value={output} onChange={e=>setOutput(e.target.value)}/></label><Button disabled={busy||!output.trim()} onClick={()=>void deliver("ExportPatch")}>{t("runtime.export")}</Button></div>}
  {s.delivery?.artifact&&<p className="text-sm break-all">{s.delivery.artifact}</p>}
 </section>;
}
