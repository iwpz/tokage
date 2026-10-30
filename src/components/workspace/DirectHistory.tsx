import {useEffect, useState} from "react";
import {useTranslation} from "react-i18next";
import {directHistory, directEvents, directReconcile, type DirectRecord} from "../../lib/direct";
import {isStreaming, useChat} from "../../stores/chat";
import {Button} from "../ui";
import {Markdown} from "../Markdown";

/** Native execution records survive loss of the UI's batched chat snapshot. */
export function DirectHistory({sessionId}: {sessionId: string}) {
  const {t}=useTranslation();
  const busy=useChat(s=>isStreaming(s.conversations[sessionId]));
  const [records,setRecords]=useState<DirectRecord[]>([]);
  const [error,setError]=useState("");
  const [selected,setSelected]=useState("");
  const [transcript,setTranscript]=useState("");
  const [revision,setRevision]=useState(0);
  const [reconciling,setReconciling]=useState(false);
  useEffect(()=>{
    let current=true;
    setRecords([]);setSelected("");setTranscript("");setError("");
    const read=()=>void directHistory(sessionId).then(rows=>{if(current){setRecords(rows);setError("");}}).catch(e=>{if(current)setError(String(e));});
    read();const timer=setInterval(read,3000);
    return()=>{current=false;clearInterval(timer);};
  },[sessionId,busy,revision]);
  useEffect(()=>{
    let current=true;setTranscript("");
    if(selected)void directEvents(selected).then(events=>{if(current)setTranscript(events.filter(e=>e.kind==="text").map(e=>(e as {text:string}).text).join(""));}).catch(e=>{if(current)setError(String(e));});
    return()=>{current=false;};
  },[selected]);
  const unresolved=records.filter(r=>r.finishedAt==null);
  if(!records.length&&!error)return null;
  return <div className="border-b border-border px-4 py-2 text-xs text-muted">
    {error&&<p role="alert" className="text-warning">{error}</p>}
    {!busy&&unresolved.length>0&&<div role="alert" className="mb-2 space-y-2"><p>{t("workspace.direct.unresolved")}</p><Button size="sm" disabled={reconciling} onClick={()=>{
      setReconciling(true);
      void (async()=>{try{for(const record of unresolved)await directReconcile(record.turnId);setRevision(n=>n+1);}catch(e){setError(String(e));}finally{setReconciling(false);}})();
    }}>{t("workspace.direct.reconcile")}</Button></div>}
    <details><summary className="cursor-pointer">{t("workspace.direct.history",{count:records.length})}</summary><div className="mt-2 max-h-80 space-y-3 overflow-auto">
      {records.map(record=><div key={record.turnId} className="space-y-1 border-b border-border pb-2"><p>{record.request.provider} · {record.request.model} · {t(`workspace.direct.states.${record.status}`)}</p><p className="break-all">{record.cwd}</p><p>{new Date(record.createdAt*1000).toLocaleString()}</p><p className="whitespace-pre-wrap break-words">{record.request.prompt}</p>{record.error&&<p className="text-warning">{record.error}</p>}<Button size="sm" onClick={()=>setSelected(record.turnId)}>{t("workspace.direct.readReply")}</Button>{selected===record.turnId&&transcript&&<Markdown text={transcript}/>}</div>)}
    </div></details>
  </div>;
}
