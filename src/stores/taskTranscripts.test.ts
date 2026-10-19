import { beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("../lib/ipc", async (original) => ({ ...await original<object>(), inTauri: true, storageWrite: vi.fn().mockResolvedValue(undefined) }));
import { storageWrite, type startChatTurn } from "../lib/ipc";
import { beginTaskTranscript, taskTranscriptEvent, finishTaskTranscript, flushTaskTranscript, transcriptRows, useTaskTranscripts } from "./taskTranscripts";
const request = (id: string) => ({ turnId: id, attemptId: id, sessionId: "session", provider: "CLAUDE_CODE", model: "haiku", cwd: "/fixture", prompt: `prompt ${id}`, mode: "direct" } as Parameters<typeof startChatTurn>[0]);
beforeEach(() => { useTaskTranscripts.setState({ entries: {} }); vi.mocked(storageWrite).mockClear(); });
describe("task session transcripts", () => {
  it("shows the sent prompt and independent parallel reply streams before completion", async () => {
    beginTaskTranscript(request("T1")); beginTaskTranscript(request("T2"));
    taskTranscriptEvent({ turnId: "T1", kind: "text", text: "first " });
    taskTranscriptEvent({ turnId: "T2", kind: "reasoning", text: "thinking" });
    taskTranscriptEvent({ turnId: "T1", kind: "text", text: "chunk" });
    const entries = useTaskTranscripts.getState().entries;
    expect(transcriptRows(entries.T1).map((r) => r.content)).toEqual(["prompt T1", "first chunk"]);
    expect(entries.T2.messages[1].text).toBe(""); expect(entries.T2.messages[1].parts).toEqual([{ type: "reasoning", text: "thinking" }]);
    expect(entries.T1.messages[1].status).toBe("streaming");
    await flushTaskTranscript("T1"); await flushTaskTranscript("T2");
    expect(vi.mocked(storageWrite).mock.calls.at(-1)?.[0].messages.every((m) => m.attemptId === "T2")).toBe(true);
  });
  it("retains tool activity and partial text on cancellation and rejects late success", async () => {
    beginTaskTranscript(request("cancelled"));
    taskTranscriptEvent({ turnId: "cancelled", kind: "toolStart", id: "read", tool: "read", title: "calc.mjs", detail: null });
    taskTranscriptEvent({ turnId: "cancelled", kind: "text", text: "partial" });
    finishTaskTranscript("cancelled", "cancelled");
    taskTranscriptEvent({ turnId: "cancelled", kind: "text", text: "late" });
    const reply = useTaskTranscripts.getState().entries.cancelled.messages[1];
    expect(reply.text).toBe("partial"); expect(reply.status).toBe("cancelled"); expect(reply.parts?.[0].type).toBe("tool");
    await flushTaskTranscript("cancelled");
  });
  it("preserves error output and a final write cannot regress to an older streaming snapshot", async () => {
    beginTaskTranscript(request("failure"));
    taskTranscriptEvent({ turnId: "failure", kind: "error", message: "OAuth revoked" });
    await flushTaskTranscript("failure");
    const messages = vi.mocked(storageWrite).mock.calls.at(-1)?.[0].messages;
    expect(messages?.[1].message).toMatchObject({ status: "error", text: "OAuth revoked" });
  });
  it("serializes a slow initial save before the final reply snapshot", async () => {
    let release!: () => void;
    vi.mocked(storageWrite).mockImplementationOnce(() => new Promise<void>((resolve) => { release = resolve; }));
    beginTaskTranscript(request("slow"));
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    taskTranscriptEvent({ turnId: "slow", kind: "text", text: "final reply" });
    finishTaskTranscript("slow", "done");
    const flush = flushTaskTranscript("slow");
    expect(storageWrite).toHaveBeenCalledOnce();
    release(); await flush;
    const last = vi.mocked(storageWrite).mock.calls.at(-1)?.[0].messages[1].message;
    expect(last).toMatchObject({ text: "final reply", status: "done" });
  });
  it("retains messages and exposes a storage failure, then supports retry", async () => {
    beginTaskTranscript(request("retry")); await flushTaskTranscript("retry");
    vi.mocked(storageWrite).mockRejectedValueOnce(new Error("disk full"));
    await flushTaskTranscript("retry");
    expect(useTaskTranscripts.getState().entries.retry.error).toContain("disk full");
    await flushTaskTranscript("retry");
    expect(useTaskTranscripts.getState().entries.retry.error).toBeUndefined();
  });
});
