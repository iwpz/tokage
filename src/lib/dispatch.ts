import { sessionCwd, useProjects } from "../stores/projects";
import { queueOf, setTurnDoneListener, streamingMessage, useChat } from "../stores/chat";
import type { TurnMode } from "./ipc";
import type { RequestMode } from "../stores/settings";
import { planTurn } from "./turn";

export type DispatchResult = "sent" | "untrusted" | "noModel" | "noSession";

/**
 * Starts a turn in an existing session. With `prefer`, the turn being replaced keeps its model
 * when the new prompt triages into the same mode, so the CLI session continues.
 */
export function dispatch(
  sessionId: string,
  text: string,
  mode: RequestMode,
  prefer?: Parameters<typeof planTurn>[2],
  explicitRoute?: TurnMode | null,
): DispatchResult {
  const project = useProjects.getState().projects.find((p) => p.sessions.some((x) => x.id === sessionId));
  if (!project) return "noSession";
  if (project.trusted !== true) return "untrusted";
  const plan = planTurn(text, mode, prefer, explicitRoute);
  if (!plan) return "noModel";
  useProjects.getState().touchSession(sessionId);
  void useChat.getState().send({ sessionId, cwd: sessionCwd(project, sessionId), text, ...plan });
  return "sent";
}

/** Sends the oldest queued message, if the session is idle. Failures leave it queued. */
function drain(sessionId: string) {
  const chat = useChat.getState();
  const conv = chat.conversations[sessionId];
  const next = queueOf(conv)[0];
  if (!next || streamingMessage(conv)) return;
  const item = chat.takeQueued(sessionId, next.id)!;
  if (dispatch(sessionId, item.text, item.mode) !== "sent") {
    useChat.getState().enqueue(sessionId, item.text, item.mode, true);
  }
}

// A cleanly finished turn hands over to the queue. Defer so the store settles first.
setTurnDoneListener((sessionId) => queueMicrotask(() => drain(sessionId)));

/**
 * "Send now": stops the running turn and sends the queued message in its place.
 * Tokage re-triages the message; if it lands in the interrupted turn's mode, the same model
 * picks up (continuing its CLI session and cache), otherwise the mode's usual model takes it.
 */
export async function sendQueuedNow(sessionId: string, id: string): Promise<DispatchResult> {
  const chat = useChat.getState();
  const running = streamingMessage(chat.conversations[sessionId]);
  const item = chat.takeQueued(sessionId, id);
  if (!item) return "noSession";
  const prefer =
    running?.ref && running.mode ? { ref: running.ref, mode: running.mode } : null;
  if (running) {
    // cancel awaits CLI exit (F2.c); only then is it safe to start another turn (F2.e).
    await useChat.getState().cancel(sessionId);
    if (streamingMessage(useChat.getState().conversations[sessionId])) {
      useChat.getState().enqueue(sessionId, item.text, item.mode, true);
      return "noSession";
    }
  }
  const result = dispatch(sessionId, item.text, item.mode, prefer);
  if (result !== "sent") useChat.getState().enqueue(sessionId, item.text, item.mode, true);
  return result;
}
