import { expect, it } from "vitest";
import { runtimeHistoryAttempts, runtimeHistoryLogs } from "./runtimeHistory";
import { runtimeUiSnapshot as snapshot, runtimeUiTranscripts as transcripts } from "../../tests/fixtures/runtime-ui";
it("projects real runtime turns into the history browser without inventing legacy task IDs or timestamps", () => {
 const [attempt] = runtimeHistoryAttempts(snapshot, transcripts);
 expect(attempt).toMatchObject({ id: "planner", providerName: "CLAUDE_CODE", modelId: "claude-haiku-5-5", requestedModel: "haiku", taskId: null, createdAt: null, tokenSource: "UNKNOWN", cliWindowIdentity: "native-thread" });
 expect(attempt.endedAt).toBe(new Date(1792720800000).toISOString());
});
it("reconstructs a selected turn's messages, original stream ordering and recorded usage", () => {
 const logs = runtimeHistoryLogs(snapshot, "planner", transcripts.planner);
 expect(logs.map(log => log.role)).toEqual(["USER", "ASSISTANT"]);
 expect(logs[1].content).toBe("# Answer\n\n**Hello**");
 expect(JSON.parse(logs[1].metaJson).usage).toMatchObject({ input: 11, output: 22, costUsd: 0.02 });
 expect(logs[1].content).not.toContain("hidden-wire-thread");
});
it("does not manufacture an executor's prompt from the planning requirement", () => {
 const s = structuredClone(snapshot); s.turns!.planner.role = "executor"; s.turns!.planner.taskId = "T1";
 const logs = runtimeHistoryLogs(s, "planner", transcripts.planner);
 expect(logs.map(log => log.role)).toEqual(["ASSISTANT"]); expect(runtimeHistoryAttempts(s, transcripts)[0].taskCode).toBe("T1");
});

it("uses durable creation time and context receipt without requiring transcript loading", () => {
 const s = structuredClone(snapshot);
 s.turns!.planner.createdAt = 1792720700;
 s.turns!.planner.usage = { inputTokens: 100000, outputTokens: 50, contextTokens: 12345, source: "Structured" };
 expect(runtimeHistoryAttempts(s, {})[0]).toMatchObject({ createdAt: new Date(1792720700000).toISOString(), contextTokens: 12345, tokenSource: "STRUCTURED" });
});
it("uses explicit transcript context tokens including zero, never cumulative input as context", () => {
 const rows = structuredClone(transcripts);
 rows.planner.push({turnId: "planner", sequence: 5, event: {kind: "done", inputTokens: 99999, outputTokens: 3, contextTokens: 0, source: "STRUCTURED", costUsd: null, premiumRequests: null}});
 expect(runtimeHistoryAttempts(snapshot, rows)[0]).toMatchObject({contextTokens: 0, tokenSource: "STRUCTURED"});
 expect(runtimeHistoryAttempts(snapshot, transcripts)[0].tokenSource).toBe("UNKNOWN");
});
