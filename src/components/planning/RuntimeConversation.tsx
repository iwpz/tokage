import { RuntimeTaskBoard } from "./RuntimeTaskBoard";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { AssistantReply, UserBubble } from "../Conversation";
import { runtimeMessage, runtimeTurns } from "../../lib/runtimeMessages";
import { useRuntime } from "../../stores/runtime";

export function RuntimeConversation({ sessionId, children }: { sessionId: string; children?: React.ReactNode }) {
  const { t } = useTranslation();
  const projection = useRuntime(s => s.runs[sessionId]);
  const end = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  useEffect(() => { if (stick.current) end.current?.scrollIntoView({ block: "end" }); }, [projection]);
  if (!projection) return null;
  const { snapshot: s, transcripts } = projection;
  return <div role="log" aria-live="polite" aria-label="Conversation stream" className="min-h-0 flex-1 overflow-y-auto" onScroll={e => { const el = e.currentTarget; stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80; }}>
    <ol aria-label={t("workspace.tabs.chat")} className="mx-auto flex max-w-3xl flex-col gap-6 px-3 py-6 sm:px-6">
      <li><UserBubble m={{ id: s.runId, role: "user", text: s.requirement }} /></li>
      {runtimeTurns(s, projection.turnOrder).map(turn => <li key={turn.id}><AssistantReply sessionId={sessionId} m={runtimeMessage(s, turn, transcripts[turn.id] ?? [])} /></li>)}
      {s.plan && <li><RuntimeTaskBoard snapshot={s} compact /></li>}
      <li>{children}</li>
    </ol>
    <div ref={end} />
  </div>;
}
