import {useState} from "react";
import {useTranslation} from "react-i18next";
import {History} from "lucide-react";
import {directSessions,type DirectRecord} from "../../lib/direct";
import {restoreDirectSession} from "../../lib/directRecovery";
import {useUi} from "../../stores/ui";
import {Button,Modal} from "../ui";

/** Explicit discovery also exposes sessions whose first UI batch never reached
 * SQLite. Deleted/archived sessions are not silently resurrected on startup. */
export function DirectRecoveryMenu({projectId,projectPath}:{projectId:string;projectPath:string}){
  const {t}=useTranslation();
  const [open,setOpen]=useState(false),[loading,setLoading]=useState(false);
  const [records,setRecords]=useState<DirectRecord[]>([]),[error,setError]=useState("");
  const [restoring,setRestoring]=useState("");
  const load=async()=>{
    setOpen(true);setLoading(true);setError("");
    try{setRecords(await directSessions(projectId,projectPath));}catch(e){setError(String(e));}finally{setLoading(false);}
  };
  const sessions=new Map<string,{first:DirectRecord;last:DirectRecord}>();
  for(const record of records){const old=sessions.get(record.sessionId);sessions.set(record.sessionId,{first:old?.first??record,last:record});}
  return <>
    <div className="flex justify-end border-b border-border px-3 py-1"><Button variant="ghost" size="sm" onClick={()=>void load()}><History className="size-3.5"/>{t("workspace.direct.recoverSessions")}</Button></div>
    <Modal open={open} onClose={()=>!restoring&&setOpen(false)} title={t("workspace.direct.recoverSessions")}>
      <div className="space-y-3 overflow-auto p-4 text-sm">
        <p className="text-muted">{t("workspace.direct.recoveryHint")}</p>
        {loading&&<p role="status">{t("runtime.loading")}</p>}
        {!loading&&!records.length&&!error&&<p>{t("workspace.direct.noRecoverableSessions")}</p>}
        {error&&<p role="alert" className="text-warning">{error}</p>}
        {[...sessions.values()].reverse().map(({first,last})=><div key={first.sessionId} className="space-y-1 border-b border-border pb-3">
          <p>{first.request.conversation?.sessionTitle??first.request.conversation?.userText??first.request.prompt.split("\n")[0]}</p>
          <p className="break-all text-xs text-muted">{last.cwd}</p>
          <p className="text-xs text-muted">{last.request.provider} · {last.request.model} · {t(`workspace.direct.states.${last.status}`)}</p>
          <Button size="sm" disabled={!!restoring} onClick={()=>{
            setRestoring(first.sessionId);setError("");
            void restoreDirectSession(projectId,first.sessionId,true).then(()=>{useUi.getState().setWorkspaceTab("chat");setOpen(false);}).catch(e=>setError(String(e))).finally(()=>setRestoring(""));
          }}>{t("workspace.direct.restoreChat")}</Button>
        </div>)}
      </div>
    </Modal>
  </>;
}
