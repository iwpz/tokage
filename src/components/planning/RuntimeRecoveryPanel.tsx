import { useTranslation } from "react-i18next";
import type { RuntimeSnapshot } from "../../lib/runtime";
import { runtimeRecovery } from "../../lib/runtimeRecovery";
import { useModelLabel } from "../../lib/useModelLabel";
import { runtimeFailure } from "../../lib/runtimeFailure";
import { Button } from "../ui";

/** Evidence stays visible even when the last durable state could not be updated. */
export function RuntimeRecoveryPanel({snapshot:s,readError,workerError,commandError,onRefresh,onShowPipeline,busy=false}:{snapshot:RuntimeSnapshot;readError?:string;workerError?:string;commandError?:string;onRefresh:()=>void;onShowPipeline?:()=>void;busy?:boolean}) {
 const {t}=useTranslation();const modelLabel=useModelLabel();const failure=runtimeFailure(s,modelLabel);const recovery=runtimeRecovery(s,readError,workerError,commandError);
 if(!recovery.kind&&!commandError)return null;
 const kind=recovery.kind??"command";
 return <section role="alert" aria-label={t("recovery.title")} className="mx-4 mt-3 max-h-[45vh] shrink-0 overflow-y-auto rounded-xl border border-warning/30 bg-warning/5 p-4 text-xs">
  <strong>{t(s.state==="Blocked"?"runtime.blockedTitle":"recovery.title")}</strong>
  {s.state==="Blocked"&&<><p className="mt-1">{failure.cleanupUnknown?t("runtime.blockedCleanupUnknown",{models:failure.cleanupModels}):failure.unknownCost?t("runtime.blockedUnknownCost",{models:failure.models}):failure.message??t("runtime.blockedMissingReason")}</p>{failure.cleanupUnknown&&<p>{t("runtime.blockedCleanupUnknownHelp")}</p>}{failure.unknownCost&&<p>{t("runtime.blockedUnknownCostHelp")}</p>}</>}<p className="mt-1">{t(`recovery.kinds.${kind}`)}</p>
  <p className="mt-2 text-muted">{t("recovery.recorded",{state:s.state,revision:s.revision})}</p>
  <p className="mt-2">{t(`recovery.help.${kind}`)}</p>{readError&&kind!=="observation"&&<p className="mt-2">{t("recovery.help.observation")}</p>}
  <div className="mt-2 flex flex-wrap gap-2"><Button size="sm" disabled={busy} onClick={onRefresh}>{t("runtime.retryRead")}</Button>{onShowPipeline&&<Button size="sm" onClick={onShowPipeline}>{t("executionBoard.viewPipelineDag")}</Button>}</div>
  <details className="mt-2"><summary>{t("runtime.technicalDetails")}</summary><pre className="whitespace-pre-wrap break-words">{[readError,workerError,commandError,recovery.failure.message].filter(Boolean).join("\n")||t("runtime.blockedMissingReason")}</pre></details>
  <details className="mt-2"><summary>{t("recovery.artifacts",{count:recovery.artifacts.length})}</summary>
   <p className="mt-2 break-all">{s.config.workspace} · {s.config.targetReference??"—"} · {s.config.targetIdentity}</p>
   {s.work?.integration&&<p className="break-all">{s.work.integration.cwd} · {s.work.integration.branch} · {s.work.integrationHead??"—"}</p>}
   {recovery.artifacts.map(a=><article key={a.id} className="mt-2 rounded border border-border p-2 break-all"><strong>{a.taskId??"—"} · {a.id}</strong><p>{a.workspace?.cwd??"—"} · {a.workspace?.branch??"—"}</p><p>{t("recovery.commits")}: {a.commit??"—"} / {a.integratedHead??"—"}</p>{a.error&&<p>{a.error}</p>}{a.gates.map((g,i)=><details key={g.id??i}><summary>{g.command??g.id??"—"} · {g.result?(g.result.passed?"PASS":"FAIL"):t("runtime.pending")} · {g.result?.exitCode??"—"} · {g.cleanupConfirmed===true?t("runtime.cleanupConfirmed"):t("runtime.cleanupPending")}</summary><pre className="whitespace-pre-wrap break-words">{g.result?.outputTail}{"\n"}{g.result?.errorMessage}</pre></details>)}{a.report!=null&&<details><summary>{t("recovery.report")}</summary><pre className="whitespace-pre-wrap break-words">{JSON.stringify(a.report,null,2)}</pre></details>}</article>)}
   {s.delivery?.candidate?.head&&<p className="mt-2 break-all">{t("runtime.candidateCommit")}: {s.delivery.candidate.head}</p>}
   {s.delivery?.artifact&&<p className="break-all">{s.delivery.artifact}</p>}
  </details>
 </section>;
}
