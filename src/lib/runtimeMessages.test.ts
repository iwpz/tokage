import { expect, it } from "vitest";
import { runtimeMessage } from "./runtimeMessages";
import type { ChatEventKind } from "./ipc";
import type { RuntimeSnapshot, RuntimeTurn } from "./runtime";
const snapshot = { config: { workspace: "/project" } } as RuntimeSnapshot;
const turn: RuntimeTurn = { id: "turn", role: "planner", taskId: null, status: "Completed", requested: { provider: "CLAUDE_CODE", requestedModel: "haiku", providerPath: null, capabilities: [], userLocked: true }, observedModel: "claude-haiku-5-5", observedProvider: "CLAUDE_CODE", costUsd: 0.02, lastSequence: 0, approval: null, error: null, ended: true, cleanupConfirmed: true };
const project = (events: ChatEventKind[], source = turn) => runtimeMessage(snapshot, source, events.map((event, i) => ({ turnId: "turn", sequence: i + 1, event })));
it("rejoins token fragments without adding whitespace and keeps reasoning separate", () => {
 const m = project([{ kind: "thread", id: "private-thread" }, { kind: "model", name: "haiku" }, { kind: "reasoning", text: "Think" }, { kind: "reasoning", text: "ing" }, { kind: "text", text: "# Pl" }, { kind: "text", text: "an\n\n**Ready**" }]);
 expect(m.text).toBe("# Plan\n\n**Ready**");
 expect(m.parts).toEqual([{ type: "reasoning", text: "Thinking" }, { type: "text", text: "# Plan\n\n**Ready**" }]);
 expect(m.reportedModel).toBe("claude-haiku-5-5");
});
it("projects tool results and file previews without leaking wire JSON into the answer", () => {
 const events: ChatEventKind[] = [{ kind: "toolStart", id: "read", tool: "read", title: "src/file", detail: null }, { kind: "toolEnd", id: "read", ok: true, output: "contents" }, { kind: "text", text: "Done" }];
 const m = project(events);
 expect(m.parts?.[0]).toMatchObject({ type: "tool", done: true, ok: true, output: "contents" });
 expect(m.text).toBe("Done"); expect(events[0]).not.toHaveProperty("done");
});
it("keeps runtime approvals out of the legacy approval dispatcher and preserves unknown cost", () => {
 const m = project([{ kind: "approvalRequest", id: "a", toolId: null, command: "npm test", level: "write", reason: "permission", recoverable: true }], { ...turn, ended: false, costUsd: null, approval: { id: "a", turnId: "turn", command: "npm test", recoverable: true, revision: 1, confirmationDigest: "digest", decision: null } });
 expect(m.status).toBe("waiting_approval"); expect(m.parts).toEqual([]); expect(m.usage).toBeUndefined();
});
it("preserves errors, quota information and exact recorded usage", () => {
 const m = project([{ kind: "quota", raw: "rate limit", retrying: false }, { kind: "done", inputTokens: 10, outputTokens: 20, costUsd: null, premiumRequests: null }, { kind: "error", message: "failed" }]);
 expect(m.status).toBe("error"); expect(m.error).toBe("failed"); expect(m.usage).toMatchObject({ input: 10, output: 20, costUsd: null });
});
it("uses durable event order rather than UUID-sorted snapshot keys", async () => {
 const { runtimeTurns } = await import("./runtimeMessages");
 const s = { ...snapshot, turns: { a: { ...turn, id: "a", role: "reviewer" }, z: { ...turn, id: "z", role: "planner" } } };
 expect(runtimeTurns(s, ["z", "z", "missing", "a"]).map(t => t.role)).toEqual(["planner", "reviewer"]);
});
