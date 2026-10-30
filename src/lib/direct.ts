import { invoke } from "@tauri-apps/api/core";
import { inTauri, type ChatEventKind } from "./ipc";

export interface DirectRecord {
  schemaVersion: number;
  turnId: string;
  sessionId: string;
  cwd: string;
  request: {prompt: string; provider: string; model: string; resume?: string | null};
  status: "prepared" | "running" | "awaiting_approval" | "completed" | "failed" | "stopped" | "unknown";
  createdAt: number;
  finishedAt: number | null;
  error: string | null;
}
export const directHistory = (sessionId: string): Promise<DirectRecord[]> =>
  inTauri ? invoke("direct_history", {sessionId}) : Promise.resolve([]);
export const directEvents = (turnId: string): Promise<ChatEventKind[]> =>
  inTauri ? invoke("direct_events", {turnId}) : Promise.resolve([]);
export const directReconcile = (turnId: string): Promise<DirectRecord> => invoke("direct_reconcile", {turnId});
