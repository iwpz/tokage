import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useChat, type ChatMessage } from "../../stores/chat";
import { Button } from "../ui";

/** Retry queues user text only. Terminal action receipts are never resurrected. */
export function CompactFailureRecovery({sessionId,message}:{sessionId:string;message:ChatMessage}) {
  const {t}=useTranslation(),[error,setError]=useState("");
  const action=message.compactAction;
  if(!action || !["failed","cancelled"].includes(action.state))return null;
  if(action.recovery)return <p role="status" className="text-xs text-subtle">{t(action.recovery==="queued"?"workspace.context.recoveryQueued":"workspace.context.recoveryIgnored")}</p>;
  const saved=action.failureReceipt==="saved",pending=action.failureReceipt==="pending";
  const recover=(choice:"queue"|"ignore")=>{setError("");try{useChat.getState().recoverCompactFailure(sessionId,message.id,choice);}catch(e){setError(String(e));}};
  return <div className="mb-2 rounded border border-border p-2 text-xs space-y-2">
    <p role="status">{t(saved?"workspace.context.failureSaved":pending?"workspace.context.failureSaving":"workspace.context.failureNotSaved")}</p>
    <p>{t("workspace.context.recoveryHint")}</p>
    <div className="flex flex-wrap gap-2">
      {!saved&&<Button size="sm" disabled={pending} onClick={()=>{setError("");void useChat.getState().retryCompactReceipt(sessionId,message.id).catch(e=>setError(String(e)));}}>{t("workspace.context.retryReceipt")}</Button>}
      <Button size="sm" disabled={!saved} onClick={()=>recover("queue")}>{t("workspace.context.requeueOriginal")}</Button>
      <Button size="sm" disabled={!saved} onClick={()=>recover("ignore")}>{t("workspace.context.ignoreFailure")}</Button>
    </div>
    {error&&<p role="alert">{error}</p>}
  </div>;
}
