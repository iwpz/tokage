import { PROGRESS_REPORT_REQUEST } from "../lib/compactContext";
import { legacyContextInputRequest } from "../lib/contextInputs";
import { acceptCompact, compactRequestKey, compactStartupKey, validatePendingCompact, type PendingCompact } from "../lib/contextPending";
import { prepareCompactTurn, queueCompactTurn, undoQueuedCompact, failCompactTurn, settleContextSavings, type CompactIntent } from "../lib/ipc";
import { demoConfig, demoIdentity } from "../lib/demoIdentity";
import { create } from "zustand";
import type { DebugRoute } from "../lib/debugProviderTypes";
import { debugConfig, plannedDebugRoute } from "../lib/debugProvider";
import { reconcileModel } from "../lib/modelReconcile";
import { canResumeNativeThread, type NativeThread } from "../lib/nativeSessionResume";
import {
  askYesNo,
  cancelChatTurn,
  getRepoDigest,
  ignoreTokageDir,
  onChatEvent,
  renderPlanningPrompt,
  resolveApproval as resolveApprovalIpc,
  saveSessionPlan,
  tokageDirExists,
  startChatTurn,
  type ChatEvent,
  type FileChangeKind,
  type ProviderId,
  type ToolKind,
  type TurnMode,
  type UserDecision,
} from "../lib/ipc";
import i18n from "../i18n";
import { resolvePlanningDomains } from "../lib/planningDomains";
import { refKey, type ModelOptions, type ModelRef } from "../lib/models";
import { authLayers, resolveMode, supportsFullAuto } from "./auth";
import { useProjects } from "./projects";
import { useProviders } from "./providers";
import { useSettings, type RequestMode } from "./settings";

export interface FileChange {
  path: string;
  absPath: string;
  /** Tokage's copy when the file lives outside the project. */
  previewPath?: string | null;
  change: FileChangeKind;
  diff: string | null;
  additions: number;
  deletions: number;
}

export interface ToolPart {
  type: "tool";
  id: string;
  tool: ToolKind;
  title: string;
  detail: string | null;
  /** null while running, or when the CLI doesn't say. */
  ok: boolean | null;
  done: boolean;
  output: string | null;
  /** Files this step changed. */
  files: string[];
}

export interface ApprovalPart {
  type: "approval";
  id: string;
  turnId?: string;
  toolId: string | null;
  command: string;
  level: string;
  reason: string;
  resolved: boolean;
  decision?: string;
}

export interface QuotaPart {
  type: "quota";
  raw: string;
  resetAt?: string | null;
  retrying: boolean;
}

/** Assistant reply content, in the order the CLI produced it. */
export type Part =
  | { type: "reasoning"; text: string }
  | { type: "text"; text: string }
  | { type: "warning"; message: string }
  | ToolPart
  | ApprovalPart
  | QuotaPart;

export interface ChatMessage {
  /** Local application notice; never a model response. */
  origin?: "tokage";
  id: string;
  role: "user" | "assistant";
  /** User: the prompt. Assistant: all reply text, for history replay. */
  text: string;
  /** Assistant only: how the turn ran. `auto` records that Tokage picked mode and model. */
  mode?: TurnMode;
  auto?: boolean;
  provider?: ProviderId;
  /** Assistant only: the picked model (merged key), for continuing on the same model. */
  ref?: ModelRef;
  model?: string;
  modelName?: string;
  /** Frozen concrete startup parameters; absent legacy records cannot authorize compaction continuity. */
  variant?: SendArgs["variant"];
  /** Report provenance and baseline captured before this extra model turn. */
  contextReport?: {sourceTurnId:string; requestId:string; baselineParams:import("../lib/contextPlanDefaults").ResolvedInWindowContextParams};
  /** Carrier receipt; completed means the turn completed, not verified native compression. */
  compactAction?: { actionId:number; method:"instruction"|"fresh"|"native"; state:"starting"|"running"|"completed"|"failed"|"cancelled"|"unknown"; preparedPayload?:string;nativeCostUsd?:number|null;nativeConfirmed?:boolean;requestMessageId?:string;failureReceipt?:"pending"|"saved"|"error";recovery?:"queued"|"ignored" };
  /** Model the CLI reported running (resolved alias, or the pick of its auto router). */
  reportedModel?: string;
  /** Per-message durable identity; never inferred from the conversation current thread. */
  nativeThreadId?: string;
  nativeThreadSessionId?: string;
  debugRoute?: DebugRoute;
  /** Working directory of the turn (project or session worktree); root for file previews. */
  cwd?: string;
  parts?: Part[];
  /** Latest change per path; diffs are cumulative over the turn. */
  files?: FileChange[];
  status?: "streaming" | "done" | "error" | "cancelled" | "waiting_approval";
  /** Plan turns: where Tokage saved the plan (relative to `cwd`) and its version. */
  plan?: { path: string; version: number } | null;
  /** Plan turns: saving the plan failed. */
  planError?: string;
  error?: string;
  usage?: {
    input: number;
    output: number;
    cacheRead?: number;
    cacheWrite?: number;
    apiCalls?: number | null;
    contextTokens?: number | null;
    costUsd?: number | null;
    premiumRequests?: number | null;
    credits?: number | null;
    source?: "STRUCTURED" | "STDOUT_REGEX" | "ESTIMATED";
  };
  startedAt?: number;
  endedAt?: number;
}

/** A message typed while a turn runs; sent when the session is free (or right away on demand). */
export interface QueuedMessage {
  id: string;
  text: string;
  /** Composer mode when it was queued; mode and model are resolved when it is sent. */
  mode: RequestMode;
  createdAt: number;
}

export interface Conversation {
  /** Explicit acceptance owned by this conversation; never copied to forks. */
  pendingCompact?: PendingCompact;
  /** Auto dismissal lasts until a new completed source turn; never copied to forks. */
  autoCompactDismissedTurnId?: string;
  messages: ChatMessage[];
  /** Waiting messages, oldest first. */
  queue?: QueuedMessage[];
  /** CLI session for the model that ran the last turn; reused only for that same model. */
  thread: NativeThread | null;
  /** Allowed command patterns for this session (P2.m3). */
  sessionAllowed?: string[];
  /**
   * Single-model mode lock for this session: planning + all task execution use this model.
   * Set when a turn is dispatched with RequestMode `"fixed"`; cleared on non-fixed dispatch.
   */
  lockedModel?: ModelRef | null;
}

export interface SendArgs {
  sessionId: string;
  cwd: string;
  text: string;
  mode: TurnMode;
  auto: boolean;
  ref: ModelRef;
  /** Concrete CLI id and parameters for the chosen options. */
  variant: { id: string; effort: string | null; fast: boolean; tier: string | null };
  modelName: string;
  /**
   * F16.b: when true (default for plan first turns), wrap `text` with
   * `renderPlanningPrompt` + RepoDigest. Set false for repair / replan prompts
   * that are already fully framed.
   */
  framePlanningPrompt?: boolean;
  /** Text sent to the CLI ahead of `text`; the conversation still shows only `text`. */
  promptPrefix?: string;
  /** One-off question: starts a fresh CLI session and leaves the session's own thread alone. */
  isolated?: boolean;
  /** Internal approval/repair follow-ups must never consume a queued user action. */
  skipContextAction?: boolean;
  contextReport?: ChatMessage["contextReport"];
  skipGenerateReport?: boolean;
}

/** Composer slots that can hold a manual model pick (includes single-model mode). */
export type ModelPickSlot = TurnMode | "fixed";

interface ChatState {
  conversations: Record<string, Conversation>;
  /** Manual model overrides per mode; null means "use the recommendation". */
  picks: Record<ModelPickSlot, ModelRef | null>;
  setPick: (mode: ModelPickSlot, ref: ModelRef | null) => void;
  /** Lock / clear the session's single-model assignment. */
  setLockedModel: (sessionId: string, ref: ModelRef | null) => void;
  /** Effort / context / thinking / fast per model (`refKey`), shared by both modes. */
  modelOptions: Record<string, ModelOptions>;
  setModelOptions: (ref: ModelRef, options: ModelOptions) => void;
  queueCompact: (sessionId:string, intent:CompactIntent, automatic?:boolean) => Promise<void>;
  cancelCompact: (sessionId:string) => Promise<void>;
  retryCompactReceipt:(sessionId:string,messageId:string)=>Promise<void>;
  recoverCompactFailure:(sessionId:string,messageId:string,choice:"queue"|"ignore")=>void;
  send: (args: SendArgs) => Promise<void>;
  cancel: (sessionId: string) => Promise<void>;
  resolveApproval: (sessionId: string, approvalId: string, decision: UserDecision) => Promise<void>;
  /** `front` puts it back at the head (a send that could not start). */
  enqueue: (sessionId: string, text: string, mode: RequestMode, front?: boolean) => void;
  updateQueued: (sessionId: string, id: string, text: string) => void;
  /** Takes a message out of the queue and returns it. */
  takeQueued: (sessionId: string, id: string) => QueuedMessage | null;
  removeSession: (sessionId: string) => void;
  postSystemMessage: (sessionId: string, text: string) => void;
  /**
   * Copies a finished conversation to a new session. With `nativeFork` the next turn branches
   * the CLI session (keeps its prompt cache); otherwise it starts fresh with the history replayed.
   */
  forkConversation: (sourceId: string, targetId: string, nativeFork: boolean) => void;
}

const EMPTY: Conversation = { messages: [], thread: null };
const uid = () => crypto.randomUUID();

/** Diffs above this are dropped from storage; the preview reads the file instead. */
const DIFF_KEEP = 256 * 1024;

/** Session reservations for context report, queue and carrier preparation. */
const contextReporting = new Set<string>();
const compactPreparing = new Set<string>();
const compactQueuing = new Set<string>();
const compactQueueCompletion = new Map<string,Promise<void>>();
const compactQueueEpoch = new Map<string,number>();
const turns = new Map<string, { sessionId: string; messageId: string; ref: ModelRef; isolated?: boolean; compactActionId?:number }>();

/** Called after a turn finishes cleanly, so the queue can move on. Errors and stops pause it. */
let onTurnDone: ((sessionId: string) => void) | null = null;
export function setTurnDoneListener(fn: (sessionId: string) => void) {
  onTurnDone = fn;
}

/** Appends streamed text to the last part when it has the same type. */
function appendText(parts: Part[], type: "reasoning" | "text", text: string): Part[] {
  const last = parts[parts.length - 1];
  if (last?.type === type) return [...parts.slice(0, -1), { type, text: last.text + text }];
  return [...parts, { type, text }];
}

/** Applies one streamed event to the assistant message. */
export function reduceChatMessage(m: ChatMessage, e: ChatEvent, sessionId?: string): ChatMessage {
  const parts = m.parts ?? [];
  switch (e.kind) {
    case "debugRoute":
      return { ...m, debugRoute: e, cwd: e.workdir ?? m.cwd };
    case "thread":
      return {...m,nativeThreadId:e.id,nativeThreadSessionId:sessionId};
    case "model":
      // Remember what an alias resolved to (`opus` → the full name) for the pickers.
      if (!m.debugRoute && m.provider && m.model) {
        const r = reconcileModel(m.provider, m.model, e.name);
        if (r.kind === "resolved") useSettings.getState().recordObservedModel(m.provider, m.model, e.name);
      }
      return { ...m, reportedModel: e.name };
    case "reasoning":
      return { ...m, parts: appendText(parts, "reasoning", e.text) };
    case "text":
      return { ...m, text: m.text + e.text, parts: appendText(parts, "text", e.text) };
    case "toolStart":
      return {
        ...m,
        parts: [
          ...parts,
          { type: "tool", id: e.id, tool: e.tool, title: e.title, detail: e.detail, ok: null, done: false, output: null, files: [] },
        ],
      };
    case "toolEnd":
      return {
        ...m,
        parts: parts.map((p) => (p.type === "tool" && p.id === e.id ? { ...p, ok: e.ok, done: true, output: e.output } : p)),
      };
    case "fileChange": {
      const change: FileChange = {
        path: e.path,
        absPath: e.absPath,
        previewPath: e.previewPath,
        change: e.change,
        diff: e.diff && e.diff.length <= DIFF_KEEP ? e.diff : null,
        additions: e.additions,
        deletions: e.deletions,
      };
      const files = m.files ?? [];
      const prev = files.find((f) => f.absPath === e.absPath);
      // A file created this turn stays "added" however often it is edited afterwards.
      if (prev?.change === "added" && change.change === "modified") change.change = "added";
      return {
        ...m,
        files: prev ? files.map((f) => (f === prev ? change : f)) : [...files, change],
        parts: e.toolId
          ? parts.map((p) =>
              p.type === "tool" && p.id === e.toolId && !p.files.includes(e.path) ? { ...p, files: [...p.files, e.path] } : p,
            )
          : parts,
      };
    }
    case "approvalRequest":
      return {
        ...m,
        status: "waiting_approval",
        parts: [
          ...parts,
          {
            type: "approval",
            id: e.id,
            turnId: e.turnId,
            toolId: e.toolId,
            command: e.command,
            level: e.level,
            reason: e.reason,
            resolved: false,
          },
        ],
      };
    case "approvalResolved":
      return {
        ...m,
        parts: parts.map((p) =>
          p.type === "approval" && p.id === e.id ? { ...p, resolved: true, decision: e.decision } : p,
        ),
      };
    case "warning":
      return {
        ...m,
        parts: [...parts, { type: "warning", message: e.message }],
      };
    case "quota":
      return {
        ...m,
        parts: [
          ...parts,
          {
            type: "quota",
            raw: e.raw,
            resetAt: e.resetAt,
            retrying: e.retrying,
          },
        ],
      };
    default:
      return m;
  }
}

/** Asks once per project whether `.tokage/` goes into `.gitignore` (P1.m6e). */
async function askGitignore(projectId: string, root: string) {
  const s = useSettings.getState();
  if (projectId in s.tokageGitignore) return;
  const yes = await askYesNo(i18n.t("workspace.tokageDir.ask"), i18n.t("workspace.tokageDir.title"));
  useSettings.getState().set("tokageGitignore", { ...useSettings.getState().tokageGitignore, [projectId]: yes });
  if (yes) await ignoreTokageDir(root);
}

/** Steps still marked running when the turn stopped. */
const settleTools = (parts: Part[] | undefined) =>
  parts?.map((p) => (p.type === "tool" && !p.done ? { ...p, done: true } : p));

export const useChat = create<ChatState>()(
  (set, get) => {
    const patch = (sessionId: string, messageId: string, fn: (m: ChatMessage) => ChatMessage) =>
      set((s) => {
        const conv = s.conversations[sessionId];
        if (!conv) return s;
        return {
          conversations: {
            ...s.conversations,
            [sessionId]: { ...conv, messages: conv.messages.map((m) => (m.id === messageId ? fn(m) : m)) },
          },
        };
      });

    const noteCompact = (sessionId:string, messageId:string, key:"workspace.context.nativeConfirmed"|"workspace.context.nativeCompleted"|"workspace.context.turnCompleted"|"workspace.context.turnFailed"|"workspace.context.turnCancelled"|"workspace.context.receiptFailed") => {
      if (get().conversations[sessionId]?.messages.some(m=>m.id===messageId)) get().postSystemMessage(sessionId,i18n.t(key));
    };
    const markCompactFailed = async (sessionId:string, messageId:string, actionId:number, cancelledBeforeSend:boolean) => {
      const receipt=get().conversations[sessionId]?.messages.find(m=>m.id===messageId)?.compactAction;
      if(receipt?.failureReceipt==="pending") return;
      patch(sessionId,messageId,m=>({...m,compactAction:m.compactAction?{...m.compactAction,failureReceipt:"pending"}:undefined}));
      try {
        await failCompactTurn(actionId,messageId,cancelledBeforeSend);
        patch(sessionId,messageId,m=>({...m,compactAction:m.compactAction?{...m.compactAction,failureReceipt:"saved"}:undefined}));
      } catch(error) {
        patch(sessionId,messageId,m=>({...m,compactAction:m.compactAction?{...m.compactAction,failureReceipt:"error"}:undefined}));
        console.warn("[context] failed to update action:",error);
        noteCompact(sessionId,messageId,"workspace.context.receiptFailed");
      }
    };

    const handle = (e: ChatEvent) => {
      const turn = turns.get(e.turnId);
      if (!turn) return;
      const { sessionId, messageId } = turn;
      switch (e.kind) {
        case "thread":
          patch(sessionId,messageId,m=>reduceChatMessage(m,e,sessionId));
          if (turn.isolated) break;
          set((s) => {
            const conv = s.conversations[sessionId] ?? EMPTY;
            return {
              conversations: { ...s.conversations, [sessionId]: { ...conv, thread: { ref: turn.ref, id: e.id, mode: conv.messages.find((m) => m.id === messageId)?.mode } } },
            };
          });
          break;
        case "approvalRequest":
          patch(sessionId, messageId, (m) => {
            const updated = reduceChatMessage(m, e);
            return {
              ...updated,
              parts: settleTools(updated.parts),
            };
          });
          break;
        case "contextCompact":
          patch(sessionId,messageId,m=>({...m,compactAction:m.compactAction?{...m.compactAction,nativeCostUsd:typeof e.receipt.costUsd==="number" && Number.isFinite(e.receipt.costUsd) && e.receipt.costUsd>=0 ? e.receipt.costUsd : null,nativeConfirmed:e.confirmed}:undefined}));
          if(e.confirmed) noteCompact(sessionId,messageId,"workspace.context.nativeConfirmed");
          break;
        case "done":
          turns.delete(e.turnId);
          patch(sessionId, messageId, (m) => ({
            ...m,
            status: "done",
            compactAction: m.compactAction ? {...m.compactAction,state:"completed"} : undefined,
            endedAt: Date.now(),
            parts: settleTools(m.parts),
            usage: {
              input: e.inputTokens,
              output: e.outputTokens,
              cacheRead: e.cacheReadTokens,
              cacheWrite: e.cacheWriteTokens,
              apiCalls: e.apiCalls,
              contextTokens: e.contextTokens,
              costUsd: e.costUsd,
              premiumRequests: e.premiumRequests,
              credits: e.credits,
              source: e.source,
            },
          }));
          if(turn.compactActionId != null) {
            const action=get().conversations[sessionId]?.messages.find(m=>m.id===messageId)?.compactAction;
            noteCompact(sessionId,messageId,action?.method==="native" && action.nativeConfirmed?"workspace.context.nativeCompleted":"workspace.context.turnCompleted");
            const cost=action?.method==="native" ? (action.nativeConfirmed && e.costUsd!=null && action.nativeCostUsd!=null ? e.costUsd+action.nativeCostUsd : null) : e.costUsd ?? null;
            void settleContextSavings(turn.compactActionId,messageId,cost).catch(error=>{
              console.warn("[context] failed to save savings:",error);
              noteCompact(sessionId,messageId,"workspace.context.receiptFailed");
            });
          }
          if(!get().conversations[sessionId]?.messages.find(m=>m.id===messageId)?.contextReport) {
            onTurnDone?.(sessionId);
            void savePlan(sessionId, messageId);
          }
          break;
        case "error":
          turns.delete(e.turnId);
          patch(sessionId, messageId, (m) => ({
            ...m,
            status: "error",
            compactAction: m.compactAction ? {...m.compactAction,state:"failed"} : undefined,
            error: e.message,
            endedAt: Date.now(),
            parts: settleTools(m.parts),
          }));
          if(turn.compactActionId != null) {
            markCompactFailed(sessionId,messageId,turn.compactActionId,false);
            noteCompact(sessionId,messageId,"workspace.context.turnFailed");
          }
          break;
        default:
          patch(sessionId, messageId, (m) => reduceChatMessage(m, e));
      }
    };
    void onChatEvent(handle);

    /** A finished Plan turn's reply becomes the session's plan in `.tokage/plans/` (P1.m6b). */
    const savePlan = async (sessionId: string, messageId: string) => {
      const m = get().conversations[sessionId]?.messages.find((x) => x.id === messageId);
      if (!m || m.mode !== "plan" || !m.cwd || !m.text.trim()) return;
      const project = useProjects.getState().projects.find((p) => p.sessions.some((x) => x.id === sessionId));
      const title = project?.sessions.find((x) => x.id === sessionId)?.title ?? "";
      try {
        const first = !(await tokageDirExists(m.cwd));
        const plan = await saveSessionPlan(m.cwd, sessionId, title, m.text);
        patch(sessionId, messageId, (x) => ({ ...x, plan }));
        if (first && project && !demoIdentity(sessionId) && useSettings.getState().debugProvider.output !== "mock") await askGitignore(project.id, project.path);
      } catch (err) {
        patch(sessionId, messageId, (x) => ({ ...x, planError: String(err) }));
      }
    };

    const setQueue = (sessionId: string, fn: (q: QueuedMessage[]) => QueuedMessage[]) =>
      set((s) => {
        const conv = s.conversations[sessionId] ?? EMPTY;
        return { conversations: { ...s.conversations, [sessionId]: { ...conv, queue: fn(conv.queue ?? []) } } };
      });

    return {
      conversations: {},
      picks: { plan: null, direct: null, fixed: null },

      setPick: (mode, ref) => set((s) => ({ picks: { ...s.picks, [mode]: ref } })),
      setLockedModel: (sessionId, ref) =>
        set((s) => {
          const conv = s.conversations[sessionId] ?? EMPTY;
          return {
            conversations: {
              ...s.conversations,
              [sessionId]: { ...conv, lockedModel: ref },
            },
          };
        }),
      modelOptions: {},
      setModelOptions: (ref, options) =>
        set((s) => ({ modelOptions: { ...s.modelOptions, [refKey(ref)]: options } })),

      retryCompactReceipt:async(sessionId,messageId)=>{
        const message=get().conversations[sessionId]?.messages.find(m=>m.id===messageId);
        if(!message?.compactAction || !["failed","cancelled"].includes(message.compactAction.state)) throw new Error(i18n.t("workspace.context.recoveryUnavailable"));
        await markCompactFailed(sessionId,messageId,message.compactAction.actionId,false);
      },
      recoverCompactFailure:(sessionId,messageId,choice)=>{
        const conv=get().conversations[sessionId],index=conv?.messages.findIndex(m=>m.id===messageId) ?? -1;
        const message=conv?.messages[index],action=message?.compactAction;
        if(!conv || !action || !["failed","cancelled"].includes(action.state) || action.failureReceipt!=="saved" || action.recovery) throw new Error(i18n.t("workspace.context.recoveryUnavailable"));
        if(choice==="queue") {
          if(conv.pendingCompact || contextReporting.has(sessionId) || compactQueuing.has(sessionId) || compactPreparing.has(sessionId) || conv.messages.some(m=>m.status==="streaming" || m.status==="waiting_approval")) throw new Error(i18n.t("workspace.context.recoveryBusy"));
          const original=action.requestMessageId ? conv.messages.find(m=>m.id===action.requestMessageId) : conv.messages[index-1];
          if(original?.role!=="user" || original.origin==="tokage" || !original.text.trim() || !message.mode) throw new Error(i18n.t("workspace.context.recoveryUnavailable"));
          patch(sessionId,messageId,m=>({...m,compactAction:{...action,recovery:"queued"}}));
          get().enqueue(sessionId,original.text,message.mode,true);
        } else {patch(sessionId,messageId,m=>({...m,compactAction:{...action,recovery:"ignored"}}));}
      },

      queueCompact: async (sessionId,intent,automatic=false) => {
        if(contextReporting.has(sessionId) || compactQueuing.has(sessionId) || compactPreparing.has(sessionId)) return;
        const conv=get().conversations[sessionId];
        const settings=useSettings.getState();
        if(!conv || conv.pendingCompact || settings.contextMode==="off" || settings.debugProvider.output!=="off" || demoIdentity(sessionId)) return;
        if(automatic && (settings.contextMode!=="auto" || conv.autoCompactDismissedTurnId===intent.scope.turnId)) return;
        if(useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId))?.trusted!==true) throw new Error("Workspace trust is required");
        const request=legacyContextInputRequest(conv,settings.cacheAware,settings.contextMinSaving,sessionId);
        if(!request) throw new Error("Current context unavailable");
        const epoch=compactQueueEpoch.get(sessionId) ?? 0;
        compactQueuing.add(sessionId);
        let finishQueue!:()=>void;
        compactQueueCompletion.set(sessionId,new Promise<void>(resolve=>{finishQueue=resolve;}));
        let actionId:number|undefined;
        try {
          const {resolveContextInputs}=await import("../lib/ipc");
          const inputs=await resolveContextInputs(request);
          if(JSON.stringify(inputs.compactIntent)!==JSON.stringify(intent)) throw new Error("Suggestion changed; review it again");
          if(automatic && inputs.automaticQueueSuppressed) return;
          const source=conv.messages.find(m=>m.id===intent.scope.turnId);
          if(!source?.variant) throw new Error("Startup parameters are missing; complete a normal turn before compaction");
          const accepted=acceptCompact(request,inputs);
          actionId=await queueCompactTurn(request,intent,automatic);
          const current=get().conversations[sessionId], live=useSettings.getState();
          const latest=legacyContextInputRequest(current,live.cacheAware,live.contextMinSaving,sessionId);
          if(!current || current.pendingCompact || !latest || compactRequestKey(latest)!==accepted.requestKey ||
             (compactQueueEpoch.get(sessionId) ?? 0)!==epoch || live.contextMode==="off" || live.debugProvider.output!=="off" ||
             useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId))?.trusted!==true ||
             (automatic && (live.contextMode!=="auto" || current.autoCompactDismissedTurnId===intent.scope.turnId))) {
            await undoQueuedCompact(actionId,sessionId); return;
          }
          const pendingCompact={...accepted,actionId,automatic,startupKey:compactStartupKey(source.variant),cwd:source.cwd};
          set(s=>({conversations:{...s.conversations,[sessionId]:{...s.conversations[sessionId],pendingCompact}}}));
          get().postSystemMessage(sessionId,i18n.t(automatic?"workspace.context.autoQueued":"workspace.context.queued"));
        } finally {compactQueuing.delete(sessionId);compactQueueCompletion.delete(sessionId);finishQueue();}
      },
      cancelCompact: async sessionId => {
        compactQueueEpoch.set(sessionId,(compactQueueEpoch.get(sessionId) ?? 0)+1);
        const conv=get().conversations[sessionId];
        if(!conv) return;
        const pending=conv.pendingCompact;
        const source=pending?.intent.scope.turnId ?? [...conv.messages].reverse().find(m=>m.role==="assistant" && m.endedAt!=null)?.id;
        set(s=>({conversations:{...s.conversations,[sessionId]:{...s.conversations[sessionId],pendingCompact:undefined,autoCompactDismissedTurnId:source}}}));
        if(!pending) return;
        try {
          if(pending.actionId!=null && !compactPreparing.has(sessionId)) await undoQueuedCompact(pending.actionId,sessionId);
          get().postSystemMessage(sessionId,i18n.t("workspace.context.queueCancelled"));
        } catch(error) {
          get().postSystemMessage(sessionId,i18n.t("workspace.context.receiptFailed"));
          throw error;
        }
      },
      send: async ({
        sessionId,
        cwd,
        text,
        mode,
        auto,
        ref,
        variant,
        modelName,
        framePlanningPrompt,
        promptPrefix,
        isolated,
        skipContextAction,
        contextReport,
        skipGenerateReport,
      }) => {
        if(contextReporting.has(sessionId) && !contextReport) {get().enqueue(sessionId,text,mode);return;}
        if(!contextReport && !skipGenerateReport && !isolated && !skipContextAction && !promptPrefix && useSettings.getState().contextGenerateReport && useSettings.getState().contextMode!=="off") {
          await compactQueueCompletion.get(sessionId);
          const before=get().conversations[sessionId], settings=useSettings.getState();
          if(before && useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId))?.trusted===true && !before.messages.some(m=>m.status==="streaming" || m.status==="waiting_approval") &&
             (before.pendingCompact || settings.contextMode==="auto") && settings.debugProvider.output==="off" && !demoIdentity(sessionId)) {
            const request=legacyContextInputRequest(before,settings.cacheAware,settings.contextMinSaving,sessionId);
            const source=before.messages.find(m=>m.id===request?.retention?.turnId);
            if(request && source && !source.contextReport && source.variant && source.model===variant.id && source.cwd===cwd &&
               compactStartupKey(source.variant)===compactStartupKey(variant) && canResumeNativeThread(before.thread,ref,mode) && !before.thread?.fork &&
               before.autoCompactDismissedTurnId!==source.id) {
              let started=true;
              contextReporting.add(sessionId);
              const decisionEpoch=compactQueueEpoch.get(sessionId) ?? 0;
              try {
                const {resolveContextInputs}=await import("../lib/ipc");
                const inputs=await resolveContextInputs(request);
                if(before.pendingCompact) validatePendingCompact(before.pendingCompact,request,ref.provider,variant.id,mode);
                if(inputs.warmNow && !inputs.automaticQueueSuppressed && inputs.compactIntent?.nextTurn.type==="FRESH_WINDOW_BRIEF" &&
                   inputs.cooldown.known && !inputs.preview.cooldownBlocked && (inputs.preview.cooldownRemainingTurns ?? 0)===0 &&
                   ["COMPACT","FRESH"].includes(inputs.preview.recommendedAction) && !inputs.compactIntent.retained.progress?.modelReport) {
                  const authorized=useSettings.getState();
                  if((compactQueueEpoch.get(sessionId) ?? 0)!==decisionEpoch || !authorized.contextGenerateReport || authorized.contextMode==="off" ||
                     ((before.pendingCompact?.automatic ?? true) && authorized.contextMode!=="auto") || authorized.debugProvider.output!=="off" || useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId))?.trusted!==true ||
                     compactRequestKey(legacyContextInputRequest(get().conversations[sessionId],authorized.cacheAware,authorized.contextMinSaving,sessionId)!)!==compactRequestKey(request)) throw new Error("Report authorization or source changed");
                  const automatic=before.pendingCompact?.automatic ?? true;
                  const epoch=decisionEpoch+(before.pendingCompact ? 1 : 0);
                  if(before.pendingCompact) await get().cancelCompact(sessionId);
                  const launchSettings=useSettings.getState();
                  if((compactQueueEpoch.get(sessionId) ?? 0)!==epoch || !launchSettings.contextGenerateReport || launchSettings.contextMode==="off" ||
                     (automatic && launchSettings.contextMode!=="auto") || launchSettings.debugProvider.output!=="off" ||
                     useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId))?.trusted!==true) throw new Error("Report launch cancelled");
                  const requestId=uid();
                  let unsubscribe=()=>{};
                  const completed=new Promise<void>((resolve,reject)=>{
                    unsubscribe=useChat.subscribe(state=>{
                      const conv=state.conversations[sessionId];
                      if(!conv) {reject(new Error("Session removed during progress report"));return;}
                      const report=conv.messages.find(m=>m.contextReport?.requestId===requestId);
                      if(report?.status==="done" && report.endedAt!=null && !report.error) resolve();
                      else if(report && (report.error || ["error","cancelled","waiting_approval"].includes(report.status ?? ""))) reject(new Error(report.error || "Progress report stopped"));
                    });
                  });
                  // Attach a handler before launch: synchronous mocked/live completion
                  // or launch failure must never produce an unhandled rejection.
                  void completed.catch(()=>{});
                  try {
                    await get().send({sessionId,cwd,text:PROGRESS_REPORT_REQUEST,mode,auto:false,ref,variant,modelName,
                      framePlanningPrompt:false,skipContextAction:true,
                      contextReport:{sourceTurnId:source.id,requestId,baselineParams:inputs.params}});
                    await completed;
                  } finally {unsubscribe();}
                  const live=useSettings.getState(),current=get().conversations[sessionId];
                  if((compactQueueEpoch.get(sessionId) ?? 0)!==epoch || !live.contextGenerateReport || live.contextMode==="off" ||
                     (automatic && live.contextMode!=="auto") || live.debugProvider.output!=="off" ||
                     useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId))?.trusted!==true) throw new Error("Progress handoff cancelled or authorization changed");
                  const next=legacyContextInputRequest(current,live.cacheAware,live.contextMinSaving,sessionId);
                  if(!next) throw new Error("Current report window unavailable");
                  const fresh=await resolveContextInputs(next);
                  if(!fresh.compactIntent || fresh.compactIntent.nextTurn.type!=="FRESH_WINDOW_BRIEF" || !["COMPACT","FRESH"].includes(fresh.preview.recommendedAction)) throw new Error("Context recommendation changed after report; review the new decision");
                  if((compactQueueEpoch.get(sessionId) ?? 0)!==epoch || !useSettings.getState().contextGenerateReport) throw new Error("Progress handoff cancelled");
                  contextReporting.delete(sessionId);
                  const queueReport=get().queueCompact(sessionId,fresh.compactIntent,automatic);
                  contextReporting.add(sessionId);
                  await queueReport;
                  if((compactQueueEpoch.get(sessionId) ?? 0)!==epoch || !useSettings.getState().contextGenerateReport) {
                    await get().cancelCompact(sessionId);
                    throw new Error("Progress handoff cancelled");
                  }
                  if(!get().conversations[sessionId]?.pendingCompact) throw new Error("Report completed but the new action was not queued");
                  contextReporting.delete(sessionId);started=false;
                  await get().send({sessionId,cwd,text,mode,auto,ref,variant,modelName,framePlanningPrompt,promptPrefix,isolated,skipContextAction,skipGenerateReport:true});
                  return;
                }
              } catch(error) {
                if(!get().conversations[sessionId]) return;
                get().enqueue(sessionId,text,mode,true);
                get().postSystemMessage(sessionId,`${i18n.t("workspace.context.prepareFailed")} ${String(error)}`);return;
              } finally {if(started)contextReporting.delete(sessionId);}
            }
          }
        }
        // Auto is opt-in and applies only to ordinary user sends. It never
        // launches a report/model just because the preview changed.
        if(!isolated && !skipContextAction && !promptPrefix && useSettings.getState().contextMode==="auto") {
          await compactQueueCompletion.get(sessionId);
          const before=get().conversations[sessionId], settings=useSettings.getState();
          if(before && !before.pendingCompact && settings.debugProvider.output==="off" && !demoIdentity(sessionId)) {
            const request=legacyContextInputRequest(before,settings.cacheAware,settings.contextMinSaving,sessionId);
            if(request && before.autoCompactDismissedTurnId!==request.retention?.turnId) {
              try {
                const {resolveContextInputs}=await import("../lib/ipc");
                const inputs=await resolveContextInputs(request);
                if(!inputs.automaticQueueSuppressed && ["COMPACT","FRESH"].includes(inputs.preview.recommendedAction) && inputs.cooldown.known && !inputs.preview.cooldownBlocked &&
                   (inputs.preview.cooldownRemainingTurns ?? 0)===0 && inputs.compactIntent && (inputs.compactIntent.nextTurn.type!=="NATIVE_COMMAND" || inputs.compactIntent.nextTurn.transportReady) && (inputs.preview.recommendedAction!=="FRESH" || inputs.compactIntent.action==="FRESH")) {
                  await get().queueCompact(sessionId,inputs.compactIntent,true);
                  await compactQueueCompletion.get(sessionId);
                }
              } catch(error) {
                get().enqueue(sessionId,text,mode,true);
                get().postSystemMessage(sessionId,`${i18n.t("workspace.context.prepareFailed")} ${String(error)}`);return;
              }
            }
          }
        }
        let conv = get().conversations[sessionId] ?? EMPTY;
        const assistantId=uid();
        let compactDraft:Awaited<ReturnType<typeof prepareCompactTurn>>|null=null;
        if(conv.pendingCompact && !isolated && !skipContextAction) {
          try {
            const settings=useSettings.getState();
            if(settings.contextMode==="off" || (conv.pendingCompact.automatic && settings.contextMode!=="auto") || settings.debugProvider.output!=="off" || demoIdentity(sessionId)) throw new Error("Context compaction is disabled; cancel the pending action");
            const owner=useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId));
            if(owner?.trusted!==true) throw new Error("Workspace trust is required");
            const request=legacyContextInputRequest(conv,settings.cacheAware,settings.contextMinSaving,sessionId);
            validatePendingCompact(conv.pendingCompact,request,ref.provider,variant.id,mode);
            const source=conv.messages.find(m=>m.id===conv.pendingCompact!.intent.scope.turnId);
            if(!source?.variant || compactStartupKey(variant)!==conv.pendingCompact.startupKey || compactStartupKey(source.variant)!==conv.pendingCompact.startupKey || conv.pendingCompact.cwd!==cwd || source.model!==variant.id || !canResumeNativeThread(conv.thread,ref,mode) || conv.thread?.fork || source.cwd!==cwd || promptPrefix) throw new Error("Model, mode, workspace or prompt changed; cancel compaction first");
            // Reserve this session before awaiting IPC; concurrent sends cannot consume the same intent.
            if(compactPreparing.has(sessionId)) throw new Error("Context compaction is already being prepared");
            compactPreparing.add(sessionId);
            try {
              compactDraft=await prepareCompactTurn(request!,conv.pendingCompact.intent,text,assistantId,conv.pendingCompact.actionId);
              if(get().conversations[sessionId]?.pendingCompact!==conv.pendingCompact || (conv.pendingCompact.automatic && useSettings.getState().contextMode!=="auto") || compactRequestKey(legacyContextInputRequest(get().conversations[sessionId],useSettings.getState().cacheAware,useSettings.getState().contextMinSaving,sessionId)!)!==conv.pendingCompact.requestKey || useSettings.getState().contextMode==="off" || useSettings.getState().debugProvider.output!=="off" || useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId))?.trusted!==true) throw new Error("Context changed while preparing compaction");
            } finally {compactPreparing.delete(sessionId);}
          } catch(error) {
            if(compactDraft?.actionId != null) {
              try { await failCompactTurn(compactDraft.actionId,assistantId,true); }
              catch(receiptError) { console.warn("[context] failed to update action:",receiptError); get().postSystemMessage(sessionId,i18n.t("workspace.context.receiptFailed")); }
            }
            get().enqueue(sessionId,text,mode,true);
            get().postSystemMessage(sessionId,`${i18n.t("workspace.context.prepareFailed")} ${String(error)}`);
            return;
          }
        }
        // Preserve unrelated diagnostic/UI writes made while prepare was awaiting IPC.
        if(compactDraft) conv=get().conversations[sessionId];
        const history = conv.messages
          .filter((m) => m.origin !== "tokage" && m.text && m.status !== "streaming")
          .map((m) => ({ role: m.role, text: m.text }));
        const sameModel = !compactDraft?.requireNewWindow && !isolated && canResumeNativeThread(conv.thread, ref, mode);
        const resume = sameModel ? conv.thread!.id : null;
        const fork = sameModel && !!conv.thread!.fork;
        const turnId = uid();
        const executionSource = demoConfig(sessionId, useSettings.getState().demoChunkDelayMs) ?? debugConfig();
        // Later turns, on any CLI, are pointed at the saved plan.
        const planFile = [...conv.messages].reverse().find((m) => m.plan && m.cwd === cwd)?.plan?.path ?? null;
        const userMessageId=uid();
        const assistant: ChatMessage = {
          id: assistantId,
          role: "assistant",
          text: "",
          mode,
          auto,
          provider: ref.provider,
          ref,
          model: variant.id,
          modelName,
          variant: {...variant},
          contextReport,
          compactAction: compactDraft ? {actionId:compactDraft.actionId,requestMessageId:userMessageId,method:compactDraft.nativeCommand?"native":compactDraft.requireNewWindow?"fresh":"instruction",state:"starting",preparedPayload:compactDraft.nativeCommand ?? compactDraft.contextPrefix} : undefined,
          cwd,
          parts: [],
          files: [],
          status: "streaming",
          debugRoute: plannedDebugRoute(ref.provider, variant.id, cwd, executionSource),
          startedAt: Date.now(),
        };
        turns.set(turnId, { sessionId, messageId: assistant.id, ref, isolated, compactActionId:compactDraft?.actionId });
        set((s) => ({
          conversations: {
            ...s.conversations,
            [sessionId]: { ...conv, pendingCompact:isolated || skipContextAction ? conv.pendingCompact:undefined, messages: [...conv.messages, { id: userMessageId, role: "user", text }, assistant] },
          },
        }));
        // Trust follows the project the session belongs to (its worktree lives elsewhere).
        const project = useProjects.getState().projects.find((p) => p.sessions.some((x) => x.id === sessionId));
        const providerInfo = useProviders.getState().providers.find((p) => p.id === ref.provider);
        const isWindows =
          typeof navigator !== "undefined" &&
          (/win(dows|32|64)/i.test(navigator.userAgent) || (navigator as any).userAgentData?.platform === "Windows");
        const fullAutoAllowed = !isWindows && supportsFullAuto(providerInfo);
        const layers = authLayers(project?.id ?? null, sessionId);
        const resolved = resolveMode(layers, fullAutoAllowed, sessionId);

        // Accepted retained material is sent through the normal turn and permission path.
        let prompt = compactDraft?.nativeCommand ? text : compactDraft ? `${compactDraft.contextPrefix}\n\n---\n\n${text}` : promptPrefix ? `${promptPrefix}\n\n---\n\n${text}` : text;

        // F16.b: Plan first turn — inject RepoDigest + format spec via planning crate.
        const shouldFramePlan =
          mode === "plan" && !resume && framePlanningPrompt !== false;
        if (shouldFramePlan) {
          try {
            const digest = await getRepoDigest(cwd);
            prompt = await renderPlanningPrompt({
              userRequest: compactDraft ? text : prompt,
              repoDigest: digest.markdown,
              domains: resolvePlanningDomains(),
            });
            if(compactDraft) prompt=`${compactDraft.contextPrefix}\n\n---\n\n${prompt}`;
          } catch (err) {
            console.warn("[chat] renderPlanningPrompt failed, sending raw request:", err);
          }
        }

        try {
          if (compactDraft) {
            const current = get().conversations[sessionId];
            const stillTrusted = useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId))?.trusted===true;
            if (!turns.has(turnId)) return;
            if (!stillTrusted || useSettings.getState().contextMode==="off" || useSettings.getState().debugProvider.output!=="off" || current?.thread?.id!==conv.thread?.id || !current.messages.some(m=>m.id===assistantId && m.status==="streaming")) throw new Error("Context or workspace authorization changed before launch");
          }
          await startChatTurn({
            turnId,
            cwd,
            prompt,
            debug: executionSource,
            mode,
            contextCompact:compactDraft?.nativeCommand ?? undefined,
            phase:contextReport?"CONTEXT":undefined,
            authMode: resolved.mode,
            provider: ref.provider,
            model: variant.id,
            effort: variant.effort,
            fast: variant.fast,
            tier: variant.tier,
            history: isolated || compactDraft?.requireNewWindow ? [] : history,
            resume,
            fork,
            planFile,
            sessionId,
            projectId: project?.id,
            sessionAllowed: conv.sessionAllowed ?? [],
            timeoutTier: mode === "plan" ? "PLANNING" : "STANDARD",
            timeoutMultiplier: useSettings.getState().timeoutMultiplier ?? 1.0,
            // CLIs only receive trust inside a workspace Tokage itself is allowed in.
            trusted:
              project?.trusted === true
                ? Object.entries(useSettings.getState().providerTrust)
                    .filter(([, v]) => v)
                    .map(([k]) => k as ProviderId)
                : [],
          });
          if (compactDraft && turns.has(turnId)) patch(sessionId,assistant.id,m=>({...m,compactAction:m.compactAction?{...m.compactAction,state:"running"}:undefined}));
        } catch (err) {
          const turn=turns.get(turnId);
          if (!turn) return;
          turns.delete(turnId);
          patch(sessionId, assistant.id, (m) => ({ ...m, status: "error", compactAction:m.compactAction?{...m.compactAction,state:"failed"}:undefined, error: String(err), endedAt: Date.now() }));
          if(turn?.compactActionId != null) {markCompactFailed(sessionId,assistant.id,turn.compactActionId,false);noteCompact(sessionId,assistant.id,"workspace.context.turnFailed");}
        }
      },

      cancel: async (sessionId) => {
        compactQueueEpoch.set(sessionId,(compactQueueEpoch.get(sessionId) ?? 0)+1);
        for (const [turnId, t] of [...turns]) {
          if (t.sessionId !== sessionId) continue;
          try {
            // Wait until the CLI process group has exited (F2.c/F2.e) before marking stopped.
            await cancelChatTurn(turnId);
            if(!turns.has(turnId)) continue;
            turns.delete(turnId);
            patch(sessionId, t.messageId, (m) => ({
              ...m,
              status: "cancelled",
              compactAction: m.compactAction ? {...m.compactAction,state:"cancelled"} : undefined,
              endedAt: Date.now(),
              parts: settleTools(m.parts),
            }));
            if(t.compactActionId != null) {markCompactFailed(sessionId,t.messageId,t.compactActionId,false);noteCompact(sessionId,t.messageId,"workspace.context.turnCancelled");}
          } catch (err) {
            patch(sessionId, t.messageId, (m) => ({
              ...m,
              error: i18n.t("workspace.chat.cancelFailed", { detail: String(err) }),
            }));
          }
        }
      },

      resolveApproval: async (sessionId, approvalId, decision) => {
        const conv = get().conversations[sessionId];
        if (!conv) return;
        const msg = conv.messages.find((m) => m.parts?.some((p) => p.type === "approval" && p.id === approvalId));
        const apr = msg?.parts?.find((p): p is ApprovalPart => p.type === "approval" && p.id === approvalId);
        if (!apr) return;

        const project = useProjects.getState().projects.find((p) => p.sessions.some((x) => x.id === sessionId));
        try {
          await resolveApprovalIpc({
            id: approvalId,
            sessionId,
            turnId: apr.turnId,
            toolId: apr.toolId,
            command: apr.command,
            decision,
            pattern: apr.command,
            projectId: project?.id,
          });
        } catch (err) {
          console.error("Failed to record approval:", err);
        }

        const sessionAllowed = [...(conv.sessionAllowed ?? [])];
        if (decision === "allow_session" || decision === "allow_project") {
          if (!sessionAllowed.includes(apr.command)) {
            sessionAllowed.push(apr.command);
          }
        }

        set((s) => {
          const c = s.conversations[sessionId];
          if (!c) return s;
          return {
            conversations: {
              ...s.conversations,
              [sessionId]: {
                ...c,
                sessionAllowed,
                messages: c.messages.map((m) => {
                  const parts = m.parts?.map((p) =>
                    p.type === "approval" && p.id === approvalId ? { ...p, resolved: true, decision } : p,
                  );
                  const hasOtherPending = parts?.some((p) => p.type === "approval" && !p.resolved);
                  return {
                    ...m,
                    parts,
                    status: hasOtherPending ? ("waiting_approval" as const) : ("done" as const),
                  };
                }),
              },
            },
          };
        });

        // Resume execution per docs/11 §4.3
        if (msg && msg.ref && msg.cwd) {
          const resumePrompt =
            decision === "deny"
              ? `User denied command: ${apr.command}. Please continue without running this command or find an alternative approach.`
              : `User approved command: ${apr.command}. Please execute it now.`;

          await get().send({
            sessionId,
            cwd: msg.cwd,
            text: resumePrompt,
            skipContextAction:true,
            mode: msg.mode ?? "direct",
            auto: msg.auto ?? false,
            ref: msg.ref,
            variant: {
              id: msg.model ?? "",
              effort: null,
              fast: false,
              tier: null,
            },
            modelName: msg.modelName ?? msg.model ?? "",
          });
        }
      },

      enqueue: (sessionId, text, mode, front) =>
        setQueue(sessionId, (q) => {
          const item = { id: uid(), text, mode, createdAt: Date.now() };
          return front ? [item, ...q] : [...q, item];
        }),

      updateQueued: (sessionId, id, text) =>
        setQueue(sessionId, (q) => q.map((x) => (x.id === id ? { ...x, text } : x))),

      takeQueued: (sessionId, id) => {
        const item = get().conversations[sessionId]?.queue?.find((x) => x.id === id) ?? null;
        if (item) setQueue(sessionId, (q) => q.filter((x) => x.id !== id));
        return item;
      },

      forkConversation: (sourceId, targetId, nativeFork) =>
        set((s) => {
          const src = s.conversations[sourceId] ?? EMPTY;
          const messages = src.messages
            .filter((m) => m.status !== "streaming")
            .map((m) => ({ ...m, id: uid() }));
          const thread = nativeFork && src.thread ? { ...src.thread, fork: true } : null;
          return { conversations: { ...s.conversations, [targetId]: { messages, thread, queue: [] } } };
        }),

      removeSession: (sessionId) =>
        set((s) => {
          const { [sessionId]: _, ...rest } = s.conversations;
          return { conversations: rest };
        }),

      postSystemMessage: (sessionId, text) =>
        set((s) => {
          const conv = s.conversations[sessionId] ?? EMPTY;
          const sysMsg: ChatMessage = {
            origin: "tokage",
            id: uid(),
            role: "assistant",
            text,
            status: "done",
            startedAt: Date.now(),
            parts: [{ type: "text", text }],
          };
          return {
            conversations: {
              ...s.conversations,
              [sessionId]: { ...conv, messages: [...conv.messages, sysMsg] },
            },
          };
        }),
    };
  },
);

/** v1 (localStorage `tokage.chat` version 1) kept tool steps as `{ label, ok }` beside the text. */
export function upgradeV1Message(m: ChatMessage): ChatMessage {
  const old = m as ChatMessage & { tools?: { label: string; ok: boolean | null }[] };
  if (m.role !== "assistant" || m.parts) return m;
  const tools: Part[] = (old.tools ?? []).map((t, i) => ({
    type: "tool",
    id: `v1-${i}`,
    tool: t.label.startsWith("edit ") ? "edit" : "command",
    title: t.label.replace(/^edit /, ""),
    detail: null,
    ok: t.ok,
    done: true,
    output: null,
    files: [],
  }));
  const { tools: _, ...rest } = old;
  return { ...rest, parts: [...tools, ...(m.text ? [{ type: "text" as const, text: m.text }] : [])], files: [] };
}

/** A turn still streaming when the app closed can never finish; mark it on load. */
export function settleOnLoad(c: Conversation): Conversation {
  return {
    ...c,
    // Pending acceptance is deliberately in-memory, including the localStorage fallback.
    pendingCompact: undefined,
    autoCompactDismissedTurnId:undefined,
    messages: c.messages.map((original) => {
      const m=original.compactAction && ["failed","cancelled"].includes(original.compactAction.state) && original.compactAction.failureReceipt!=="saved"
        ? {...original,compactAction:{...original.compactAction,failureReceipt:"error" as const}} : original;
      return m.status === "streaming" ? { ...m, status: "cancelled" as const, compactAction:m.compactAction?{...m.compactAction,state:"unknown" as const}:undefined, parts: settleTools(m.parts) } : m.status === "waiting_approval" && m.compactAction ? {...m, compactAction:{...m.compactAction,state:"unknown" as const}} : m;
    }),
  };
}

export function isStreaming(conv: Conversation | undefined) {
  return !!conv?.messages.some((m) => m.status === "streaming");
}

/** The assistant message of the running turn, if any. */
export function streamingMessage(conv: Conversation | undefined) {
  return conv?.messages.find((m) => m.status === "streaming") ?? null;
}

const NO_QUEUE: QueuedMessage[] = [];
export const queueOf = (conv: Conversation | undefined) => conv?.queue ?? NO_QUEUE;

/** Stable empty messages — never `?? []` inside a Zustand selector (v5 identity loop). */
const NO_MESSAGES: ChatMessage[] = [];
export const messagesOf = (conv: Conversation | undefined) => conv?.messages ?? NO_MESSAGES;

/**
 * Splits a reply into the work leading up to the answer (reasoning, narration, tool steps)
 * and the answer itself: the text after the last step.
 */
export function splitReply(parts: Part[]): { process: Part[]; answer: string } {
  let cut = parts.length;
  while (cut > 0 && parts[cut - 1].type === "text") cut--;
  return {
    process: parts.slice(0, cut),
    answer: parts
      .slice(cut)
      .map((p) => (p.type === "text" ? p.text : ""))
      .join(""),
  };
}
