import { contextHistorySourceIds } from "../../lib/contextActionHistory";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listContextActionHistory, type ContextActionHistoryRow } from "../../lib/ipc";
import { useChat } from "../../stores/chat";
import { Button } from "../ui";

export function ContextActionHistory({sessionId}:{sessionId:string}) {
  const {t}=useTranslation();
  const conv=useChat(s=>s.conversations[sessionId]);
  const [open,setOpen]=useState(false),[revision,refresh]=useState(0),[rows,setRows]=useState<ContextActionHistoryRow[]>([]);
  const [loading,setLoading]=useState(false),[error,setError]=useState("");
  const ids=contextHistorySourceIds(conv,sessionId);
  const idsKey=JSON.stringify(ids);
  const receiptsKey=JSON.stringify([conv?.pendingCompact?.actionId,conv?.messages.filter(m=>m.compactAction).map(m=>[m.id,m.compactAction?.state,m.compactAction?.failureReceipt])]);
  useEffect(()=>{
    if(!open)return;
    let current=true;setLoading(true);setError("");setRows([]);
    void listContextActionHistory(sessionId,JSON.parse(idsKey)).then(result=>{if(current)setRows(result);}).catch(e=>{if(current)setError(String(e));}).finally(()=>{if(current)setLoading(false);});
    return()=>{current=false;};
  },[open,sessionId,idsKey,receiptsKey,revision]);
  return <details className="mx-3 my-1 rounded border border-border p-2 text-xs" open={open} onToggle={e=>setOpen(e.currentTarget.open)}>
    <summary className="cursor-pointer">{t("workspace.context.actionHistory")}</summary>
    {open&&<div className="mt-2 space-y-2">
      <p>{t("workspace.context.historyHint")}</p>
      <Button size="sm" disabled={loading} onClick={()=>refresh(n=>n+1)}>{t("workspace.context.refreshHistory")}</Button>
      {loading?<p role="status">{t("workspace.context.historyLoading")}</p>:error?<p role="alert">{error}</p>:rows.length===0?<p>{t("workspace.context.historyEmpty")}</p>:<ol className="space-y-2">{rows.map(row=><li key={row.id} className="rounded bg-hover p-2">
        <p>#{row.id} · {row.kind} · {row.status} · {row.trigger}{row.method?` · ${row.method}`:""}</p>
        <p className="break-all">{row.attemptId}{row.newAttemptId?` → ${row.newAttemptId}`:""}</p>
        {row.createdAt&&<p>{row.createdAt}</p>}
        {row.actualSavingUsd!=null&&<p>{t("workspace.context.historyEstimate",{amount:row.actualSavingUsd.toFixed(4)})}</p>}
      </li>)}</ol>}
    </div>}
  </details>;
}
