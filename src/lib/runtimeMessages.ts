import type { ChatMessage, Part, ToolPart } from "../stores/chat";
import type { ProviderId } from "./ipc";
import type { RuntimeSnapshot, RuntimeTurn, TranscriptEvent } from "./runtime";

/** Read-only presentation projection. Stream fragments are appended, never line-separated. */
export function runtimeMessage(snapshot: RuntimeSnapshot, turn: RuntimeTurn, rows: TranscriptEvent[]): ChatMessage {
  const parts: Part[] = [];
  const files: NonNullable<ChatMessage["files"]> = [];
  let text = "";
  let usage: ChatMessage["usage"];
  let error = turn.error ?? undefined;
  const append = (type: "text" | "reasoning", value: string) => {
    const last = parts.at(-1);
    if (last?.type === type) last.text += value;
    else parts.push({ type, text: value });
  };
  for (const { event: e } of rows) {
    switch (e.kind) {
      case "text": text += e.text; append("text", e.text); break;
      case "reasoning": append("reasoning", e.text); break;
      case "toolStart": parts.push({ type: "tool", id: e.id, tool: e.tool, title: e.title, detail: e.detail, ok: null, done: false, output: null, files: [] }); break;
      case "toolEnd": {
        const tool = parts.find((p): p is ToolPart => p.type === "tool" && p.id === e.id);
        if (tool) { tool.ok = e.ok; tool.done = true; tool.output = e.output; }
        break;
      }
      case "fileChange": {
        const { kind: _, toolId, ...file } = e;
        const index = files.findIndex(f => f.path === file.path);
        if (index < 0) files.push(file); else files[index] = file;
        const tool = parts.find((p): p is ToolPart => p.type === "tool" && p.id === toolId);
        if (tool && !tool.files.includes(file.path)) tool.files.push(file.path);
        break;
      }
      case "warning": parts.push({ type: "warning", message: e.message }); break;
      case "quota": parts.push({ type: "quota", raw: e.raw, resetAt: e.resetAt, retrying: e.retrying }); break;
      case "error": error = e.message; break;
      case "done": usage = { input: e.inputTokens, output: e.outputTokens, cacheRead: e.cacheReadTokens, cacheWrite: e.cacheWriteTokens, costUsd: e.costUsd, premiumRequests: e.premiumRequests, credits: e.credits }; break;
      // Auth decisions belong to runtime's explicit controls, never the legacy chat dispatcher.
      default: break;
    }
  }
  return {
    id: turn.id, role: "assistant", text, parts, files, cwd: snapshot.config.workspace,
    provider: turn.requested.provider as ProviderId, model: turn.requested.requestedModel,
    reportedModel: turn.observedModel ?? undefined, mode: turn.role === "planner" ? "plan" : "direct",
    status: error ? "error" : turn.approval && !turn.approval.decision ? "waiting_approval" : !turn.ended ? "streaming" : turn.status.toLowerCase().includes("cancel") ? "cancelled" : "done",
    error, usage: usage ?? (turn.costUsd == null ? undefined : { input: 0, output: 0, costUsd: turn.costUsd }),
  };
}

/** Durable event order, independent of the UUID sorting used by the native snapshot map. */
export function runtimeTurns(snapshot: RuntimeSnapshot, order: readonly string[] = []): RuntimeTurn[] {
  const turns = snapshot.turns ?? {};
  const ids = [...new Set([...order, ...Object.keys(turns)])];
  return ids.flatMap(id => turns[id] ? [turns[id]] : []);
}
