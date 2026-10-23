import type { AttemptRow, ChatLogRow } from "./ipc";
import type { RuntimeSnapshot, TranscriptEvent } from "./runtime";
import { runtimeMessage, runtimeTurns } from "./runtimeMessages";

/** View DTOs only: no history/attempt rows are inserted into the legacy database. */
export function runtimeHistoryAttempts(s: RuntimeSnapshot, transcripts: Record<string, TranscriptEvent[]>, order: readonly string[] = []): AttemptRow[] {
  return runtimeTurns(s, order).map(turn => {
    const message = runtimeMessage(s, turn, transcripts[turn.id] ?? []);
    return {
      id: turn.id, mainSessionId: s.runId, taskId: null, taskCode: turn.taskId,
      role: turn.role.toUpperCase(), providerName: turn.observedProvider ?? turn.requested.provider,
      modelId: turn.observedModel ?? turn.requested.requestedModel, requestedProvider: turn.requested.provider,
      requestedModel: turn.requested.requestedModel, cliWindowIdentity: turn.threadId ?? null,
      execMode: "SHARED_RUNTIME", workdir: s.config.workspace, status: turn.status,
      baselineTokens: 0, contextTokens: message.usage?.input ?? 0, tokenSource: "UNKNOWN",
      turnCount: 1, touchedFilesJson: JSON.stringify(message.files?.map(file => file.path) ?? []),
      createdAt: null, endedAt: turn.finishedAt == null ? null : new Date(turn.finishedAt * 1000).toISOString(),
    };
  });
}
export function runtimeHistoryLogs(s: RuntimeSnapshot, turnId: string, rows: TranscriptEvent[]): ChatLogRow[] {
  const turn = s.turns?.[turnId];
  if (!turn) return [];
  const message = runtimeMessage(s, turn, rows);
  return [
    // Only the planning request is recorded in Snapshot. Executor/reviewer internal prompts are not fabricated.
    ...(turn.role === "planner" ? [{ id: 0, messageId: `${turnId}:requirement`, mainSessionId: s.runId, attemptId: turnId, seq: 0, turnIndex: 0, role: "USER", content: s.requirement, contentTruncated: false, metaJson: "{}", createdAt: "" }] : []),
    { id: 1, messageId: turnId, mainSessionId: s.runId, attemptId: turnId, seq: 1, turnIndex: 0, role: "ASSISTANT", content: message.text, contentTruncated: false, metaJson: JSON.stringify(message), createdAt: "" },
  ];
}
