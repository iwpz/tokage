import {useEffect,useRef,useState} from "react";
import {useTranslation} from "react-i18next";
import {runtimeRouteTaskMessage,runtimeRequest,runtimeSubmit,type RuntimeTaskMessagePreview,type RuntimeCommand} from "../../lib/runtime";
import {useRuntime} from "../../stores/runtime";
import {useProjects} from "../../stores/projects";
import {useUi} from "../../stores/ui";
import {RuntimeTaskContext} from "./RuntimeTaskContext";
import {Button} from "../ui";
/** Sends only an authoritative, reviewed task route. Never enters the legacy direct engine. */
export function RuntimeComposer({sessionId}:{sessionId:string}) {
 const {t}=useTranslation();
 const s=useRuntime(state=>state.runs[sessionId]?.snapshot);
 const readError=useRuntime(state=>state.readErrors[sessionId]);
 const workerError=useRuntime(state=>state.errors[sessionId]);
 const [text,setText]=useState("");const [target,setTarget]=useState("");
 const [result,setResult]=useState<{key:string;preview?:RuntimeTaskMessagePreview;error?:string}|null>(null);
 const [error,setError]=useState("");const [busy,setBusy]=useState(false);const sending=useRef(false);
 const [refresh,setRefresh]=useState(0);
 const key=JSON.stringify([sessionId,text,target,s?.revision,s?.configHash,s?.approvedDigest]);
 useEffect(()=>{
  let active=true;
  if(!s?.plan||!text.trim())return;
  const timer=setTimeout(()=>{
   void runtimeRouteTaskMessage(sessionId,text,target||null).then(preview=>{if(active)setResult({key,preview});}).catch(e=>{if(active)setResult({key,error:String(e)});});
  },200);
  return()=>{active=false;clearTimeout(timer);};
 },[key,refresh]);
 if(!s)return null;
 const preview=result?.key===key?result.preview:undefined;
 const current=preview?.revision===s.revision&&preview.configHash===s.configHash&&preview.planDigest===s.approvedDigest;
 const canSend=!!current&&!!preview?.taskId&&!!preview.sourceTurn&&!preview.unavailableReason&&!busy&&!readError&&!workerError;
 const trusted=()=>useProjects.getState().projects.find(p=>p.sessions.some(x=>x.id===sessionId))?.trusted===true;
 const contextCommand=async(command:RuntimeCommand)=>{
  if(sending.current||!trusted()){setError(t("settings.cli.trust.blocked"));return;}
  sending.current=true;setBusy(true);setError("");
  try{await useRuntime.getState().command(sessionId,command);}catch(e){setError(String(e));}finally{sending.current=false;setBusy(false);}
 };
 const submit=async()=>{
  if(sending.current||!canSend||!preview)return;
  if(!trusted()){setError(t("settings.cli.trust.blocked"));return;}
  sending.current=true;setBusy(true);setError("");const message=text;
  try{
   await runtimeSubmit(runtimeRequest(s,{type:"AppendRoutedTaskMessage",payload:{message,explicitTaskId:target||null,targetTaskId:preview.taskId,sourceTurn:preview.sourceTurn,configHash:preview.configHash,confirmationDigest:preview.planDigest}}));
   setText(value=>value===message?"":value);setResult(null);
   await useRuntime.getState().refresh(sessionId);
  }catch(e){setError(String(e));}finally{sending.current=false;setBusy(false);}
 };
 return <section aria-label={t("taskMessage.title")} className="mx-auto w-full max-w-3xl mt-3 sm:mt-4 space-y-2 px-3 pb-4 sm:px-6">
  <label className="flex items-center gap-2 text-xs">{t("taskMessage.target")}<select aria-label={t("taskMessage.target")} value={target} disabled={busy} onChange={e=>{setTarget(e.target.value);setError("");}} className="h-7 min-w-0 max-w-full rounded-lg surface-card px-2 text-xs text-fg">
   <option value="">{t("taskMessage.auto")}</option>
   {target&&!s.plan?.plan.tasks.some(task=>task.id===target)&&<option value={target}>{t("taskMessage.missing",{id:target})}</option>}
   {s.plan?.plan.tasks.map(task=><option key={task.id} value={task.id}>{task.id} · {task.title} · {s.execution?.tasks[task.id]?.state??"PENDING"}</option>)}
  </select></label>
  <textarea aria-label={t("taskMessage.text")} placeholder={t("taskMessage.text")} value={text} onChange={e=>setText(e.target.value)} onKeyDown={e=>{if(e.key==="Enter"&&(e.metaKey||e.ctrlKey)){e.preventDefault();void submit();}}} className="block w-full resize-y rounded-xl surface-card p-3 text-[13.5px] text-fg placeholder:text-subtle focus:outline-none" rows={3}/>
  <p className="text-xs text-muted">{t("taskMessage.hint")}</p>
  {preview&&<div role="status" className="text-xs"><p>{t("taskMessage.route",{route:preview.taskId??preview.route.type})} · {preview.route.reason}</p>{preview.unavailableReason&&<p className="text-warning">{preview.unavailableReason}</p>}{!preview.taskId&&<p>{t(preview.route.type==="INCREMENTAL_REPLAN"?"taskMessage.replan":"taskMessage.direct")}</p>}</div>}
  {(error||(result?.key===key?result.error:"")||readError||workerError)&&<p role="alert" className="text-xs text-warning">{error||(result?.key===key?result.error:"")||readError||workerError}</p>}
  <div className="flex gap-2"><Button size="sm" disabled={!canSend} onClick={()=>void submit()}>{t("taskMessage.send")}</Button><Button size="sm" disabled={busy} onClick={()=>{setRefresh(v=>v+1);void useRuntime.getState().refresh(sessionId).catch(e=>setError(String(e)));}}>{t("runtime.retryRead")}</Button><Button size="sm" variant="ghost" onClick={()=>useUi.getState().setWorkspaceTab("pipeline")}>{t("workspace.tabs.pipeline")}</Button></div>
  {current&&preview?.taskId&&<RuntimeTaskContext key={`${sessionId}:${preview.taskId}`} snapshot={s} taskId={preview.taskId} busy={busy||!!readError||!!workerError} onCommand={command=>void contextCommand(command)}/>}
 </section>;
}
