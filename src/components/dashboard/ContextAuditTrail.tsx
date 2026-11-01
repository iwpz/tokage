import {useState} from "react";
import {useTranslation} from "react-i18next";
import type {ContextAuditAction} from "../../lib/ipc";
import {formatUsd} from "../../lib/accountingDisplay";
export function ContextAuditTrail({rows}:{rows:ContextAuditAction[]}) {
 const {t}=useTranslation();const [kind,setKind]=useState("ALL"),[query,setQuery]=useState("");
 const visible=rows.filter(r=>(kind==="ALL"||r.kind===kind)&&[r.runId,r.taskId,r.attemptId,r.sourceTurn,r.sourceThread,r.targetThread,r.windowOrigin,r.receiptKey].some(v=>v?.toLowerCase().includes(query.toLowerCase())));
 return <details className="rounded border border-border p-3"><summary>{t("contextAudit.title",{count:rows.length})}</summary>
  <p className="mt-2 text-muted">{t("contextAudit.hint")}</p>
  <div className="mt-2 flex flex-wrap gap-2"><label>{t("contextAudit.kind")} <select className="rounded border border-border bg-panel p-1" value={kind} onChange={e=>setKind(e.target.value)}><option value="ALL">{t("contextAudit.all")}</option><option>CONTEXT</option><option>REUSE</option></select></label>
   <label>{t("contextAudit.filter")} <input className="rounded border border-border bg-transparent p-1" value={query} onChange={e=>setQuery(e.target.value)}/></label></div>
  {!visible.length&&<p className="mt-2">{t("contextAudit.empty")}</p>}
  <ol className="mt-3 space-y-3">{visible.map(r=><li key={r.key} className="rounded border border-border p-2 break-all space-y-1">
   <strong>{r.kind} · {t(`contextAudit.status.${r.status}`,{defaultValue:r.status})}</strong><p>{r.reason}</p>
   <p>{r.runId} / {r.taskId??"—"} / {r.attemptId}</p>
   <p>{t("contextAudit.source")}: {r.sourceTurn??"—"} · {r.sourceThread??"—"}</p>
   <p>{t("contextAudit.target")}: {r.attemptId} · {r.targetThread??"—"} · {t("contextAudit.origin")}: {r.windowOrigin??"—"}</p>
   <p>{t("accounting.reported")}: {formatUsd(r.reportedUsd)} · {t("accounting.estimate")}: {formatUsd(r.estimatedUsd)}</p>
   <p>{t("contextAudit.settlement")}: {t(`contextAudit.settlementStatus.${r.settlement}`,{defaultValue:r.settlement})}</p>
   <p>{t("contextAudit.forecast",{baseline:formatUsd(r.baselineUsd,4),delta:formatUsd(r.forecastDeltaUsd,4)})}</p>
   <p>{t("contextAudit.noMeasuredSaving")}</p>
   <p>{r.receiptKey??"—"} · {r.evidenceDigest}</p>
   <details><summary>{t("accounting.basis")}</summary><pre className="whitespace-pre-wrap">{JSON.stringify(r.parameters,null,2)}</pre></details>
  </li>)}</ol>
 </details>;
}
