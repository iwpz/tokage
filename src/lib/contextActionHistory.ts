import type { Conversation } from "../stores/chat";

/** Source ids from one native window only; incoming FRESH links are resolved by storage. */
export function contextHistorySourceIds(conv:Conversation|undefined,sessionId:string):string[] {
  const thread=conv?.thread;
  if(!conv || !thread)return [];
  return [...new Set(conv.messages.filter(m=>m.role==="assistant" && m.nativeThreadSessionId===sessionId && m.nativeThreadId===thread.id &&
    (m.ref?.provider ?? m.provider)===thread.ref.provider && (m.ref?.model ?? m.model)===thread.ref.model && (!thread.mode || m.mode===thread.mode)).map(m=>m.id))].slice(-512);
}
