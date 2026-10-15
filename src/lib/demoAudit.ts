import { create } from "zustand";
import type { ChatEvent, ChatRequest } from "./ipc";
import { demoIdentity } from "./demoIdentity";

export interface DemoTurnAudit {
  sessionId: string;
  taskCode?: string | null;
  route?: string;
  inputTokens?: number;
  outputTokens?: number;
  apiCalls?: number | null;
  costUsd?: number | null;
  ended: number;
  cancelled?: boolean;
  error?: string;
}
export const useDemoAudit = create<{ turns: Record<string, DemoTurnAudit> }>(() => ({ turns: {} }));
export function registerDemoTurn(request: ChatRequest) {
  if (!demoIdentity(request.sessionId)) return;
  useDemoAudit.setState(s => ({ turns: { ...s.turns, [request.turnId]: { sessionId: request.sessionId!, taskCode: request.taskCode, ended: 0 } } }));
}
export function auditDemoEvent(event: ChatEvent) {
  const turn = useDemoAudit.getState().turns[event.turnId];
  if (!turn) return;
  const patch: Partial<DemoTurnAudit> = event.kind === "debugRoute" ? { route: `${event.actualProvider}/${event.actualModel}` }
    : event.kind === "done" ? { inputTokens: event.inputTokens, outputTokens: event.outputTokens, apiCalls: event.apiCalls, costUsd: event.costUsd }
    : event.kind === "error" ? { error: event.message } : {};
  if (!Object.keys(patch).length) return;
  useDemoAudit.setState(s => ({ turns: { ...s.turns, [event.turnId]: { ...turn, ...patch } } }));
}
export function auditDemoEnded(event: { turnId: string; cancelled: boolean }) {
  const turn = useDemoAudit.getState().turns[event.turnId];
  if (turn) useDemoAudit.setState(s => ({ turns: { ...s.turns, [event.turnId]: { ...turn, ended: turn.ended + 1, cancelled: event.cancelled } } }));
}
