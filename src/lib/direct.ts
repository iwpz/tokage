import { invoke } from "@tauri-apps/api/core";
import { inTauri, type ChatEventKind, type ChatRequest, type ApprovalRecord } from "./ipc";
import type {ModelRef} from "./models";
import type {SessionWorktree} from "../stores/projects";
import type {ChatMessage} from "../stores/chat";

export interface DirectConversationMetadata {
  userMessageId:string;
  assistantMessageId:string;
  userText:string;
  ref:ModelRef;
  projectPath:string;
  sessionTitle:string;
  worktree?:SessionWorktree;
  isolated?:boolean;
  contextReport?:ChatMessage["contextReport"];
}

export interface DirectRecord {
  previousTurnId?:string|null;
  schemaVersion: number;
  turnId: string;
  sessionId: string;
  cwd: string;
  request: Pick<ChatRequest,"prompt"|"provider"|"model"> & Partial<ChatRequest>;
  status: "prepared" | "running" | "awaiting_approval" | "completed" | "failed" | "stopped" | "unknown";
  createdAt: number;
  finishedAt: number | null;
  error: string | null;
}
export interface DirectSnapshot {
  sessionId:string;
  active:boolean;
  tip:string|null;
  turns:{record:DirectRecord;events:ChatEventKind[];partialTail:boolean}[];
  approvals:ApprovalRecord[];
  canonicalProjectPath?:string|null;
  canonicalExpectedCwd?:string|null;
}
export const directHistory = (sessionId: string): Promise<DirectRecord[]> =>
  inTauri ? invoke("direct_history", {sessionId}) : Promise.resolve([]);
export const directEvents = (turnId: string): Promise<ChatEventKind[]> =>
  inTauri ? invoke("direct_events", {turnId}) : Promise.resolve([]);
export const directReconcile = (turnId: string): Promise<DirectRecord> => invoke("direct_reconcile", {turnId});
export const directRecovery = (sessionId:string,projectId?:string,projectPath?:string,expectedCwd?:string):Promise<DirectSnapshot> => inTauri ? invoke("direct_recovery",{sessionId,projectId:projectId??null,projectPath:projectPath??null,expectedCwd:expectedCwd??null}):Promise.resolve({sessionId,active:false,tip:null,turns:[],approvals:[]});
export const directSessions = (projectId:string,projectPath:string):Promise<DirectRecord[]> => inTauri ? invoke("direct_sessions",{projectId,projectPath}):Promise.resolve([]);
