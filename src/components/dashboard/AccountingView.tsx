import { ContextAuditTrail } from "./ContextAuditTrail";
import { useTranslation } from "react-i18next";
import type { DashboardAccounting } from "../../lib/ipc";
import { formatUsd } from "../../lib/accountingDisplay";
import { Card } from "../ui";
export function AccountingCostCard({data}:{data?:DashboardAccounting|null}) {
 const {t}=useTranslation();
 return <Card className="p-5"><h2 className="text-xs text-subtle">{t("accounting.reported")}</h2>
  <p className="mt-3 text-3xl font-bold">{formatUsd(data?.totals.reportedUsd)}</p>
  {data && <><p className="mt-2 text-xs">{t("accounting.knownSubtotal",{amount:formatUsd(data.totals.knownReportedUsd),count:data.totals.unknownUsdReceipts})}</p>
   <p className="mt-2 text-xs">{t("accounting.estimate")}: {formatUsd(data.totals.estimatedUsd)}</p>
   <p className="mt-2 text-xs text-muted">{t("accounting.phaseNote")}</p>
   <p className="mt-2 text-xs text-muted">{t("contextAudit.unknownTokens",{count:data.totals.unknownTokenReceipts??0})}</p>
   <ul className="mt-3 space-y-1 text-xs">{Object.entries(data.phases).map(([phase,total])=><li key={phase}>{phase}: {formatUsd(total.reportedUsd)} · {t("accounting.unknown",{count:total.unknownUsdReceipts})}</li>)}</ul></>}
 </Card>;
}
export function CounterfactualCard({data}:{data?:DashboardAccounting|null}) {
 const {t}=useTranslation();
 return <Card className="p-5"><h2 className="text-xs text-subtle">{t("accounting.counterfactual")}</h2>
  <p className="mt-3 text-3xl font-bold">{data?.comparableCount ?? "—"}</p>
  <p className="mt-3 text-xs text-muted">{t("accounting.noCombinedSaving")}</p>
  {data && <p className="mt-2 text-xs">{t("accounting.comparisons",{count:data.comparisons.length})}</p>}
 </Card>;
}
export function AccountingEvidence({data}:{data?:DashboardAccounting|null}) {
 const {t}=useTranslation();
 if(!data)return null;
 return <section className="space-y-3 text-xs">
  <ContextAuditTrail rows={data.contextActions??[]}/>
  <p className="text-muted">{t("accounting.scope",{unknown:data.unknownDateReceipts,legacy:data.legacyUnkeyedReceipts})}</p>
  <p>{t("accounting.human",{seconds:data.knownHumanSeconds,unknown:data.unknownHumanEffort})}</p>
  <details className="rounded border border-border p-3"><summary>{t("accounting.receipts",{count:data.receipts.length})}</summary>
   <ul className="mt-3 space-y-3">{data.receipts.map(r=><li key={r.key} className="break-all">
    <strong>{r.provider} / {r.model} · {r.phase} · {r.status}</strong>
    <p>{t("accounting.reported")}: {formatUsd(r.reportedUsd)} · {t("accounting.estimate")}: {formatUsd(r.estimatedUsd)}</p>
    <p>{t("accounting.nativeUnits")}: credits {r.usage?.credits ?? "—"} · premium requests {r.usage?.premiumRequests ?? "—"}</p>
    <p>{t("accounting.tokenSource")}: {r.source} · {r.usage ? `${r.usage.inputTokens} / ${r.usage.cacheReadTokens} / ${r.usage.cacheWriteTokens} / ${r.usage.outputTokens}` : "—"}</p>
    <p>{t("contextAudit.attribution")}: {r.attributionBasis ?? "—"}{r.baselineTurn ? ` · ${r.baselineTurn}` : ""}</p>
    <p>{t("contextAudit.turnTokens")}: {r.attributedUsage ? `${r.attributedUsage.inputTokens} / ${r.attributedUsage.cacheReadTokens} / ${r.attributedUsage.cacheWriteTokens} / ${r.attributedUsage.outputTokens}` : "—"}</p>
    <p>{r.origin} · {r.modelBasis} · {r.estimateBasis}</p>
    <p>{r.key} · run {r.runId ?? "—"} · task {r.taskId ?? "—"} · turn {r.turnId ?? "—"}</p>
    <p>{r.evidenceDigest} · {r.observedAt == null ? "—" : `${r.observedAt} s (Unix)`}</p>
    {r.priceBasis&&<details><summary>{t("accounting.basis")}</summary><pre className="whitespace-pre-wrap">{JSON.stringify(r.priceBasis,null,2)}</pre></details>}
    {r.duplicateSources.length>0&&<p>{t("accounting.dedup")}: {r.duplicateSources.join(", ")}</p>}
   </li>)}</ul>
  </details>
  {data.comparisons.length>0&&<details className="rounded border border-border p-3"><summary>{t("accounting.counterfactual")}</summary>
   <ul className="mt-3 space-y-3">{data.comparisons.map(c=><li key={c.id} className="break-all"><strong>#{c.id} {c.kind} · {c.status}</strong><p>{t("accounting.comparisonAmounts",{baseline:formatUsd(c.baselineUsd),actual:formatUsd(c.actualUsd),delta:formatUsd(c.deltaUsd)})}</p><p>{c.reference ?? "—"} · session {c.sessionId ?? "—"} · plan {c.planId ?? "—"}</p><details><summary>{t("accounting.basis")}</summary><pre className="whitespace-pre-wrap">{c.detailJson}</pre></details></li>)}</ul>
  </details>}
  {data.humanEffort.length>0&&<details className="rounded border border-border p-3"><summary>{t("accounting.humanEvidence")}</summary><ul>{data.humanEffort.map(h=><li key={`${h.runId}:${h.attemptId}`} className="break-all">{h.runId} / {h.taskId ?? "—"} / {h.attemptId} · {h.reportedSeconds ?? "—"} s · {h.commandId}</li>)}</ul></details>}
 </section>;
}
