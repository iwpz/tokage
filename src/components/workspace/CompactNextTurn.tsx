import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useChat } from "../../stores/chat";
import { useSettings } from "../../stores/settings";
import type { ResolvedContextInputs } from "../../lib/contextInputs";
import { Button } from "../ui";

/** Queue only; actual execution remains attached to the next ordinary user turn. */
export function CompactNextTurn({sessionId,inputs}:{sessionId:string;inputs:ResolvedContextInputs|null}) {
  const {t}=useTranslation();
  const pending=useChat(s=>s.conversations[sessionId]?.pendingCompact);
  const dismissed=useChat(s=>s.conversations[sessionId]?.autoCompactDismissedTurnId);
  const mode=useSettings(s=>s.contextMode);
  const [busy,setBusy]=useState(false),[error,setError]=useState("");
  const lastAttempt=useRef("");
  const intent=inputs?.compactIntent;
  const eligible=!!inputs && ["COMPACT","FRESH"].includes(inputs.preview.recommendedAction) && inputs.cooldown.known && !inputs.preview.cooldownBlocked && (inputs.preview.cooldownRemainingTurns ?? 0)===0 && !!intent && (intent.nextTurn.type!=="NATIVE_COMMAND" || intent.nextTurn.transportReady) && (inputs.preview.recommendedAction!=="FRESH" || intent.action==="FRESH");
  const key=JSON.stringify([sessionId,mode,eligible?intent:null,dismissed,inputs?.automaticQueueSuppressed]);
  useEffect(()=>{
    if(pending && (mode==="off" || (pending.automatic && mode!=="auto"))) {
      void useChat.getState().cancelCompact(sessionId).catch(e=>setError(String(e))); return;
    }
    if(inputs?.automaticQueueSuppressed || mode!=="auto" || !eligible || !intent || pending || dismissed===intent.scope.turnId || lastAttempt.current===key) return;
    lastAttempt.current=key;
    let current=true;
    void useChat.getState().queueCompact(sessionId,intent,true).catch(e=>{if(current)setError(String(e));});
    return()=>{current=false;};
  },[key,pending,mode,sessionId,eligible,intent,dismissed,inputs?.automaticQueueSuppressed]);
  const cancel=()=>{setError("");void useChat.getState().cancelCompact(sessionId).catch(e=>setError(String(e)));};
  if(pending) return <div role="status" className="px-3 py-2 text-xs flex items-center gap-2"><span>{t(pending.automatic?"workspace.context.autoQueued":"workspace.context.queued")}</span><Button size="sm" onClick={cancel}>{t("workspace.context.cancelQueued")}</Button>{error&&<span role="alert">{error}</span>}</div>;
  if(!inputs || !intent || !["COMPACT","FRESH"].includes(inputs.preview.recommendedAction) || !inputs.cooldown.known || inputs.preview.cooldownBlocked || (inputs.preview.cooldownRemainingTurns ?? 0)>0) return error?<p role="alert">{error}</p>:null;
  if(inputs.preview.recommendedAction==="FRESH" && intent.action!=="FRESH") return null;
  if(intent.nextTurn.type==="NATIVE_COMMAND" && !intent.nextTurn.transportReady) return <div role="status" className="px-3 py-2 text-xs">{t("workspace.context.nativeDeferred")}</div>;
  return <div className="px-3 py-2 text-xs flex flex-wrap items-center gap-2"><span>{t(intent.nextTurn.type==="NATIVE_COMMAND"?"workspace.context.nativeNext":intent.nextTurn.type==="FRESH_WINDOW_BRIEF"?"workspace.context.freshNext":"workspace.context.instructionNext")}</span><Button size="sm" disabled={busy} onClick={async()=>{setBusy(true);setError("");try{await useChat.getState().queueCompact(sessionId,intent);}catch(e){setError(String(e));}finally{setBusy(false);}}}>{t("workspace.context.queueNext")}</Button>{mode==="auto"&&!inputs.automaticQueueSuppressed&&dismissed!==intent.scope.turnId&&<Button size="sm" onClick={cancel}>{t("workspace.context.skipAuto")}</Button>}{error&&<span role="alert">{error}</span>}</div>;
}
