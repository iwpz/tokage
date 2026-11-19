import { useEffect, useState } from "react";
import { legacyContextInputRequest } from "../../lib/contextInputs";
import { getChatLogs } from "../../lib/ipc";
import { useContextInputs } from "../../lib/useContextInputs";
import { useModelLabel } from "../../lib/useModelLabel";
import type { ChatMessage } from "../../stores/chat";
import { useExecution } from "../../stores/execution";
import { useSettings } from "../../stores/settings";
import { useTaskTranscripts } from "../../stores/taskTranscripts";
import { ContextUsageRow } from "./ContextUsagePopover";

export function LegacyTaskContextUsage({ sessionId }: { sessionId: string }) {
  const pipeline = useExecution(state => state.pipelines[sessionId]);
  return pipeline?.tasks.map(task => {
    const attemptId = pipeline.taskTurnIds?.[task.id] ?? pipeline.completionReceipts?.[task.id]?.turnId;
    return attemptId && (
      <LegacyTaskUsageRow key={attemptId} attemptId={attemptId} label={`${task.id} · ${task.title}`} />
    );
  });
}

function LegacyTaskUsageRow({ attemptId, label }: { attemptId: string; label: string }) {
  const modelLabel = useModelLabel();
  const cacheAware = useSettings(state => state.cacheAware);
  const minSaving = useSettings(state => state.contextMinSaving);
  const live = useTaskTranscripts(state => state.entries[attemptId]);
  const [stored, setStored] = useState<ChatMessage[]>([]);
  useEffect(() => {
    if (live) return;
    let current = true;
    void getChatLogs(attemptId).then(rows => {
      if (!current) return;
      setStored(rows.flatMap(row => {
        try {
          const message = JSON.parse(row.metaJson) as ChatMessage | null;
          return message?.id && typeof message.text === "string" ? [message] : [];
        } catch {
          return [];
        }
      }));
    }).catch(() => {});
    return () => { current = false; };
  }, [attemptId, !!live]);

  const messages = live?.messages ?? stored;
  const request = legacyContextInputRequest({ messages, thread: null }, cacheAware, minSaving);
  const inputs = useContextInputs(request, request != null);
  const model = request ? modelLabel({ provider: request.provider, requestedModel: request.model }) : undefined;

  return <ContextUsageRow label={label} model={model} inputs={inputs} />;
}
