import { create } from "zustand";
import {
  askYesNo,
  cancelChatTurn,
  ignoreTokageDir,
  onChatEvent,
  saveSessionPlan,
  tokageDirExists,
  startChatTurn,
  type ChatEvent,
  type FileChangeKind,
  type ProviderId,
  type ToolKind,
  type TurnMode,
} from "../lib/ipc";
import i18n from "../i18n";
import { refKey, sameRef, type ModelOptions, type ModelRef } from "../lib/models";
import { useProjects } from "./projects";
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

/** Assistant reply content, in the order the CLI produced it. */
export type Part =
  | { type: "reasoning"; text: string }
  | { type: "text"; text: string }
  | ToolPart;

export interface ChatMessage {
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
  /** Model the CLI reported running (resolved alias, or the pick of its auto router). */
  reportedModel?: string;
  /** Working directory of the turn (project or session worktree); root for file previews. */
  cwd?: string;
  parts?: Part[];
  /** Latest change per path; diffs are cumulative over the turn. */
  files?: FileChange[];
  status?: "streaming" | "done" | "error" | "cancelled";
  /** Plan turns: where Tokage saved the plan (relative to `cwd`) and its version. */
  plan?: { path: string; version: number } | null;
  /** Plan turns: saving the plan failed. */
  planError?: string;
  error?: string;
  usage?: { input: number; output: number; costUsd?: number | null; premiumRequests?: number | null };
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
  messages: ChatMessage[];
  /** Waiting messages, oldest first. */
  queue?: QueuedMessage[];
  /** CLI session for the model that ran the last turn; reused only for that same model. */
  thread: { ref: ModelRef; id: string; fork?: boolean } | null;
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
}

interface ChatState {
  conversations: Record<string, Conversation>;
  /** Manual model overrides per mode; null means "use the recommendation". */
  picks: Record<TurnMode, ModelRef | null>;
  setPick: (mode: TurnMode, ref: ModelRef | null) => void;
  /** Effort / context / thinking / fast per model (`refKey`), shared by both modes. */
  modelOptions: Record<string, ModelOptions>;
  setModelOptions: (ref: ModelRef, options: ModelOptions) => void;
  send: (args: SendArgs) => Promise<void>;
  cancel: (sessionId: string) => Promise<void>;
  /** `front` puts it back at the head (a send that could not start). */
  enqueue: (sessionId: string, text: string, mode: RequestMode, front?: boolean) => void;
  updateQueued: (sessionId: string, id: string, text: string) => void;
  /** Takes a message out of the queue and returns it. */
  takeQueued: (sessionId: string, id: string) => QueuedMessage | null;
  removeSession: (sessionId: string) => void;
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

/** turnId → (sessionId, assistant message id); in-memory only. */
const turns = new Map<string, { sessionId: string; messageId: string; ref: ModelRef }>();

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
function reduce(m: ChatMessage, e: ChatEvent): ChatMessage {
  const parts = m.parts ?? [];
  switch (e.kind) {
    case "model":
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

    const handle = (e: ChatEvent) => {
      const turn = turns.get(e.turnId);
      if (!turn) return;
      const { sessionId, messageId } = turn;
      switch (e.kind) {
        case "thread":
          set((s) => {
            const conv = s.conversations[sessionId] ?? EMPTY;
            return {
              conversations: { ...s.conversations, [sessionId]: { ...conv, thread: { ref: turn.ref, id: e.id } } },
            };
          });
          break;
        case "done":
          turns.delete(e.turnId);
          patch(sessionId, messageId, (m) => ({
            ...m,
            status: "done",
            endedAt: Date.now(),
            parts: settleTools(m.parts),
            usage: { input: e.inputTokens, output: e.outputTokens, costUsd: e.costUsd, premiumRequests: e.premiumRequests },
          }));
          onTurnDone?.(sessionId);
          void savePlan(sessionId, messageId);
          break;
        case "error":
          turns.delete(e.turnId);
          patch(sessionId, messageId, (m) => ({
            ...m,
            status: "error",
            error: e.message,
            endedAt: Date.now(),
            parts: settleTools(m.parts),
          }));
          break;
        default:
          patch(sessionId, messageId, (m) => reduce(m, e));
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
        if (first && project) await askGitignore(project.id, project.path);
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
      picks: { plan: null, direct: null },

      setPick: (mode, ref) => set((s) => ({ picks: { ...s.picks, [mode]: ref } })),
      modelOptions: {},
      setModelOptions: (ref, options) =>
        set((s) => ({ modelOptions: { ...s.modelOptions, [refKey(ref)]: options } })),

      send: async ({ sessionId, cwd, text, mode, auto, ref, variant, modelName }) => {
        const conv = get().conversations[sessionId] ?? EMPTY;
        const history = conv.messages
          .filter((m) => m.text && m.status !== "streaming")
          .map((m) => ({ role: m.role, text: m.text }));
        const sameModel = !!conv.thread && sameRef(conv.thread.ref, ref);
        const resume = sameModel ? conv.thread!.id : null;
        const fork = sameModel && !!conv.thread!.fork;
        const turnId = uid();
        // Later turns, on any CLI, are pointed at the saved plan.
        const planFile = [...conv.messages].reverse().find((m) => m.plan && m.cwd === cwd)?.plan?.path ?? null;
        const assistant: ChatMessage = {
          id: uid(),
          role: "assistant",
          text: "",
          mode,
          auto,
          provider: ref.provider,
          ref,
          model: variant.id,
          modelName,
          cwd,
          parts: [],
          files: [],
          status: "streaming",
          startedAt: Date.now(),
        };
        turns.set(turnId, { sessionId, messageId: assistant.id, ref });
        set((s) => ({
          conversations: {
            ...s.conversations,
            [sessionId]: { ...conv, messages: [...conv.messages, { id: uid(), role: "user", text }, assistant] },
          },
        }));
        // Trust follows the project the session belongs to (its worktree lives elsewhere).
        const project = useProjects.getState().projects.find((p) => p.sessions.some((x) => x.id === sessionId));
        try {
          await startChatTurn({
            turnId,
            cwd,
            prompt: text,
            mode,
            provider: ref.provider,
            model: variant.id,
            effort: variant.effort,
            fast: variant.fast,
            tier: variant.tier,
            history: resume ? [] : history,
            resume,
            fork,
            planFile,
            // CLIs only receive trust inside a workspace Tokage itself is allowed in.
            trusted:
              project?.trusted === true
                ? Object.entries(useSettings.getState().providerTrust)
                    .filter(([, v]) => v)
                    .map(([k]) => k as ProviderId)
                : [],
          });
        } catch (err) {
          turns.delete(turnId);
          patch(sessionId, assistant.id, (m) => ({ ...m, status: "error", error: String(err), endedAt: Date.now() }));
        }
      },

      cancel: async (sessionId) => {
        for (const [turnId, t] of turns) {
          if (t.sessionId !== sessionId) continue;
          turns.delete(turnId);
          await cancelChatTurn(turnId);
          patch(sessionId, t.messageId, (m) => ({
            ...m,
            status: "cancelled",
            endedAt: Date.now(),
            parts: settleTools(m.parts),
          }));
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
    messages: c.messages.map((m) =>
      m.status === "streaming" ? { ...m, status: "cancelled" as const, parts: settleTools(m.parts) } : m,
    ),
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
