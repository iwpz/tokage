import { expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { RuntimeSnapshot } from "../../lib/runtime";
const fixtures = vi.hoisted(() => ({ state: {} as any }));
vi.mock("../../stores/runtime", () => ({ useRuntime: (select: (state: unknown) => unknown) => select(fixtures.state), observeRuntime: vi.fn() }));
import "../../i18n";
import { RuntimeView } from "./RuntimeView";
const snapshot = { schemaVersion: 1, runId: "run", revision: 1, state: "AwaitingApproval", requirement: "User requirement", configHash: "hash", config: { workspace: "/project", concurrency: 1, models: {}, repairPolicy: "manual", blockUnknownCost: true }, plan: { plan: { rawMarkdown: "# Readable plan\n\n**Formatted**", tasks: [] }, confirmationDigest: "digest" }, turns: { planner: { id: "planner", role: "planner", requested: { provider: "CLAUDE_CODE", requestedModel: "haiku" }, observedModel: "claude-haiku-5-5", ended: true, status: "Completed", costUsd: null } } } as unknown as RuntimeSnapshot;
fixtures.state = { runs: { run: { snapshot, transcripts: { planner: [{ turnId: "planner", sequence: 1, event: { kind: "thread", id: "hidden-wire-thread" } }, { turnId: "planner", sequence: 2, event: { kind: "text", text: "# An" } }, { turnId: "planner", sequence: 3, event: { kind: "text", text: "swer\n\n**Hello**" } }] } } }, errors: {} };
it("renders the original chat bubbles and Markdown with reassembled streamed text", () => {
 const html = renderToString(<RuntimeView sessionId="run" view="chat" />);
 expect(html).toContain('role="log"'); expect(html).toContain("User requirement"); expect(html).toContain("Answer</h3>"); expect(html).toContain("<strong>Hello</strong>"); expect(html).not.toContain("hidden-wire-thread"); expect(html).not.toContain("<textarea");
});
it("keeps plan reading and pipeline controls in their own tabs", () => {
 const plan = renderToString(<RuntimeView sessionId="run" view="plan" />);
 const pipeline = renderToString(<RuntimeView sessionId="run" view="pipeline" />);
 expect(plan).toContain("Readable plan</h3>"); expect(plan).toContain("<strong>Formatted</strong>");
 expect(pipeline).not.toContain("Readable plan"); expect(pipeline).not.toContain('role="log"');
 expect(pipeline).toContain("Full Auto");
});
