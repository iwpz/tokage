import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useChat } from "../../stores/chat";
import type { ResolvedContextInputs } from "../../lib/contextInputs";
import { Button } from "../ui";

/** Explicit acceptance only. Runtime/task windows and auto actions remain unavailable. */
export function CompactNextTurn({sessionId,inputs}:{sessionId:string;inputs:ResolvedContextInputs|null}) {
  const {t}=useTranslation();
  const pending=useChat(s=>s.conversations[sessionId]?.pendingCompact);
  const [busy,setBusy]=useState(false),[error,setError]=useState("");
  const intent=inputs?.compactIntent;
  if(pending) return <div role="status" className="px-3 py-2 text-xs flex items-center gap-2"><span>{t("workspace.context.queued")}</span><Button size="sm" onClick={()=>useChat.getState().cancelCompact(sessionId)}>{t("workspace.context.cancelQueued")}</Button></div>;
  if(!inputs || inputs.preview.recommendedAction!=="COMPACT" || !inputs.cooldown.known || inputs.preview.cooldownBlocked || (inputs.preview.cooldownRemainingTurns ?? 0)>0 || !intent) return null;
  if(intent.nextTurn.type==="NATIVE_COMMAND") return <div role="status" className="px-3 py-2 text-xs">{t("workspace.context.nativeDeferred")}</div>;
  return <div className="px-3 py-2 text-xs flex flex-wrap items-center gap-2"><span>{t(intent.nextTurn.type==="FRESH_WINDOW_BRIEF"?"workspace.context.freshNext":"workspace.context.instructionNext")}</span><Button size="sm" disabled={busy} onClick={async()=>{setBusy(true);setError("");try{await useChat.getState().queueCompact(sessionId,intent);}catch(e){setError(String(e));}finally{setBusy(false);}}}>{t("workspace.context.queueNext")}</Button>{error&&<span role="alert">{error}</span>}</div>;
}
