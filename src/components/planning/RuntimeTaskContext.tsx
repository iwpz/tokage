import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ResolvedContextInputs } from "../../lib/contextInputs";
import { runtimeTaskContext, type RuntimeSnapshot, type RuntimeCommand } from "../../lib/runtime";
import { runtimeRecovery } from "../../lib/runtimeRecovery";
import { contextCarrierState, taskContextHistory } from "../../lib/runtimeTaskContext";
import { ContextDecisionDetails } from "../workspace/ContextDecisionDetails";
import { Badge, Button } from "../ui";

export function RuntimeTaskContext({snapshot:s,taskId,busy=false,onCommand}:{snapshot:RuntimeSnapshot;taskId:string;busy?:boolean;onCommand?:(command:RuntimeCommand)=>void}) {
 const {t}=useTranslation();
 const {source,current,rows}=taskContextHistory(s,taskId);
 const key=JSON.stringify([s.runId,taskId,source?.id,s.configHash,s.approvedDigest,s.revision]);
 const [result,setResult]=useState<{key:string;inputs?:ResolvedContextInputs;error?:string}|null>(null);
 const [refresh,setRefresh]=useState(0);
 const task=s.execution?.tasks[taskId];
 const queued=task?.contextActions?.at(-1)?.status==="PREPARED"?task.contextActions.at(-1):null;
 useEffect(()=>{
  let active=true;
  if(!source?.threadId||!source.ended||!source.cleanupConfirmed||s.config.contextPolicy==="off")return;
  void runtimeTaskContext(s.runId,taskId,source.id).then(inputs=>{if(active)setResult({key,inputs});}).catch(e=>{if(active)setResult({key,error:String(e)});});
  return()=>{active=false;};
 },[key,refresh]);
 const inputs=result?.key===key?result.inputs:null;
 const canQueue=!!onCommand&&!busy&&!runtimeRecovery(s).effectsPending&&s.state==="Blocked"&&task?.state==="BLOCKED"&&!task.deviation&&!queued&&!task.pendingFollowup&&!!inputs?.cooldown.known&&!inputs.preview.cooldownBlocked&&["COMPACT","FRESH"].includes(inputs.preview.recommendedAction)&&!!inputs.compactIntent;
 return <section aria-label={t("taskContext.title")} className="space-y-2 rounded border border-border p-2 text-xs">
  <h3 className="font-semibold">{t("taskContext.title")} · {s.config.contextPolicy}</h3>
  <p className="break-all">{t("taskContext.window",{id:current?.threadId??t("runtime.unknownValue")})}</p>
  <p className="text-muted">{t("taskContext.hint")}</p>
  {s.config.contextPolicy==="off"?<p>{t("taskContext.off")}</p>:<>
   {inputs&&<><p>{t("taskContext.suggestion",{action:inputs.preview.recommendedAction})}</p><ContextDecisionDetails inputs={inputs}/></>}
   {result?.key===key&&result.error&&<p role="alert">{result.error}</p>}
   <Button size="sm" disabled={busy||!source?.threadId} onClick={()=>setRefresh(n=>n+1)}>{t("workspace.context.refreshHistory")}</Button>
   {onCommand&&<Button size="sm" disabled={!canQueue} onClick={()=>{if(source&&inputs)onCommand({type:"QueueTaskContext",payload:{taskId,sourceTurn:source.id,confirmationDigest:s.approvedDigest,configHash:s.configHash,recommendedAction:inputs.preview.recommendedAction}});}}>{t("taskContext.queue")}</Button>}
  </>}
  {queued&&onCommand&&<Button size="sm" disabled={busy} onClick={()=>onCommand({type:"CancelTaskContext",payload:{taskId,sourceTurn:queued.sourceTurn}})}>{t("taskContext.cancel")}</Button>}
  {rows.some(r=>r.turn?.contextPreparation?.status==="PREPARED"&&!r.turn.ended)&&onCommand&&<Button size="sm" disabled={busy} onClick={()=>onCommand({type:"CancelRun"})}>{t("taskContext.stop")}</Button>}
  <details><summary>{t("workspace.context.actionHistory")} ({rows.length})</summary>
   {!rows.length&&<p>{t("workspace.context.historyEmpty")}</p>}
   <ol className="mt-2 space-y-2">{rows.map(({preparation:p,turn,stale},i)=><li key={`${p.sourceTurn}:${p.targetTurn}:${i}`} className="space-y-1 rounded bg-hover p-2">
    <p><Badge size="xs">{t(`taskContext.status.${p.status==="PREPARED"&&turn?.ended?"FAILED":p.status}`)}</Badge> · {p.preview.preview.recommendedAction} · {p.policy}</p>
    <p className="break-all">{p.sourceTurn} → {p.targetTurn}</p>
    {stale&&<p className="text-warning">{t("taskContext.stale")}</p>}
    <p>{t("taskContext.applies",{value:p.applies?t("taskContext.yes"):t("taskContext.no")})}</p>
    {p.applies&&<p>{t(p.requireNewWindow?"taskContext.freshTransport":"taskContext.sameTransport")}</p>}
    {turn&&<p>{t(`taskContext.carrier.${contextCarrierState(turn)}`)}</p>}
    <p>{p.nativeConfirmed?t("taskContext.nativeConfirmed"):t("taskContext.nativeUnknown")}</p>
    {p.failure&&<p role="alert">{p.failure}</p>}
    {turn?.error&&<p role="alert">{turn.error}</p>}
    <ContextDecisionDetails inputs={p.preview}/>
   </li>)}</ol>
  </details>
 </section>;
}
