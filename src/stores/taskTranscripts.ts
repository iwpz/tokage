/** Per-attempt transcripts. They are never replayed as user input in main chat. */
import { create } from "zustand";
import { inTauri, storageWrite, type ChatEvent, type ChatLogRow, type StoredMessageRow, type startChatTurn } from "../lib/ipc";
import { reduceChatMessage, type ChatMessage } from "./chat";

type Request = Parameters<typeof startChatTurn>[0];
interface Transcript { request: Request; messages: ChatMessage[]; error?: string }
export const useTaskTranscripts = create<{ entries: Record<string, Transcript> }>(() => ({ entries: {} }));
const timers = new Map<string, ReturnType<typeof setTimeout>>();
const writes = new Map<string, Promise<void>>();
function publish(id: string, entry: Transcript) {
  useTaskTranscripts.setState((s) => ({ entries: { ...s.entries, [id]: entry } }));
}
export function transcriptRows(entry: Transcript): ChatLogRow[] {
  return entry.messages.map((message, seq) => ({
    id: -(seq + 1), messageId: message.id, mainSessionId: entry.request.sessionId!, attemptId: entry.request.turnId,
    seq, turnIndex: 0, role: message.role.toUpperCase(), content: message.text, contentTruncated: false,
    metaJson: JSON.stringify(message), createdAt: new Date(message.startedAt ?? 0).toISOString(),
  }));
}
export async function flushTaskTranscript(id: string): Promise<void> {
  clearTimeout(timers.get(id)); timers.delete(id);
  const entry = useTaskTranscripts.getState().entries[id];
  if (!entry || !inTauri) return;
  const messages: StoredMessageRow[] = entry.messages.map((message, seq) => ({
    attemptId: id, sessionId: entry.request.sessionId!, seq, turnIndex: 0,
    provider: entry.request.provider, model: entry.request.model, mode: "direct", message,
  }));
  const write = (writes.get(id) ?? Promise.resolve()).catch(() => {}).then(() => storageWrite({
    projects: [], sessions: [], sessionChat: [], messages, settings: [], providers: null, customPack: null,
    clearCustomPack: false, deleteSessions: [], deleteProjects: [],
  }));
  writes.set(id, write);
  try {
    await write;
    const current = useTaskTranscripts.getState().entries[id];
    if (writes.get(id) === write && current?.error) publish(id, { ...current, error: undefined });
  } catch (error) {
    const current = useTaskTranscripts.getState().entries[id];
    if (current) publish(id, { ...current, error: String(error) });
    console.warn("Task transcript persistence failed", error);
  } finally { if (writes.get(id) === write) writes.delete(id); }
}
function schedule(id: string) {
  if (!timers.has(id)) timers.set(id, setTimeout(() => { void flushTaskTranscript(id); }, 250));
}
export function beginTaskTranscript(request: Request) {
  if (!request.sessionId || !request.attemptId || request.attemptId !== request.turnId || useTaskTranscripts.getState().entries[request.turnId]) return;
  const startedAt = Date.now();
  publish(request.turnId, { request, messages: [
    { id: `${request.turnId}:user`, role: "user", text: request.prompt, startedAt },
    { id: `${request.turnId}:assistant`, role: "assistant", text: "", parts: [], status: "streaming", provider: request.provider, model: request.model, cwd: request.cwd, startedAt },
  ] });
  void flushTaskTranscript(request.turnId);
}
export function finishTaskTranscript(id: string, status: "done" | "error" | "cancelled") {
  const entry = useTaskTranscripts.getState().entries[id];
  if (!entry || ["done", "error", "cancelled"].includes(entry.messages[1].status ?? "")) return;
  publish(id, { ...entry, messages: [entry.messages[0], { ...entry.messages[1], status, endedAt: Date.now() }] });
  void flushTaskTranscript(id);
}
export function taskTranscriptEvent(event: ChatEvent) {
  const entry = useTaskTranscripts.getState().entries[event.turnId];
  if (!entry || ["done", "error", "cancelled"].includes(entry.messages[1].status ?? "")) return;
  let assistant = reduceChatMessage(entry.messages[1], event);
  if (event.kind === "error") assistant = { ...assistant, text: assistant.text + (assistant.text ? "\n\n" : "") + event.message, status: "error", endedAt: Date.now() };
  if (event.kind === "done") assistant = { ...assistant, status: "done", endedAt: Date.now() };
  if (event.kind === "approvalResolved") assistant = { ...assistant, status: "streaming" };
  publish(event.turnId, { ...entry, messages: [entry.messages[0], assistant] });
  if (event.kind === "error" || event.kind === "done") void flushTaskTranscript(event.turnId);
  else schedule(event.turnId);
}
