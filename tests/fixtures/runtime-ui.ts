import type { RuntimeSnapshot, TranscriptEvent } from "../../src/lib/runtime";
export const runtimeUiSnapshot: RuntimeSnapshot = {
 schemaVersion: 1, runId: "run", revision: 7, state: "AwaitingApproval", requirement: "User requirement", configHash: "hash",
 config: { source: "desktop", policyVersion: "1", workspace: "/project", targetIdentity: "target", scope: ["src"], commandPolicyHash: "policy", trustWorkspace: true, permissionMode: "workspace-write", concurrency: 1, budgetUsd: null, blockUnknownCost: true, timeoutSeconds: 60, repairPolicy: "manual", contextPolicy: "off", confirmedCommands: [], customPackHashes: [], coAuthor: false, models: { executor: { provider: "CLAUDE_CODE", requestedModel: "haiku", userLocked: true, providerPath: null, capabilities: [] } } },
 plan: { confirmationDigest: "digest", issues: [], assignments: { T1: { provider: "CLAUDE_CODE", requestedModel: "haiku", userLocked: true, providerPath: null, capabilities: [] } }, plan: { spec: { raw: "# Spec", goal: "Structured feature goal", inScope: ["src"], outOfScope: ["tests"], decisions: ["Preserve API"] }, contracts: [{ id: "C1", title: "Public contract", content: "**API contract**" }], tasks: [{ id: "T1", title: "Implement feature", domain: "general", goal: "Meet the API contract", depends_on: [], contracts: ["C1"], write_scope: ["src/file.ts"], deliverables: ["Updated source"], size: "S", difficulty: 1, acceptance: { commands: ["npm test"], criteria: ["Feature works"] } }], rawMarkdown: "# Raw plan\n\nUnstructured fallback" } },
 turns: { planner: { id: "planner", role: "planner", taskId: null, requested: { provider: "CLAUDE_CODE", requestedModel: "haiku", userLocked: true, providerPath: null, capabilities: [] }, observedModel: "claude-haiku-5-5", observedProvider: "CLAUDE_CODE", ended: true, status: "Completed", costUsd: 0.02, lastSequence: 4, approval: null, cleanupConfirmed: true, error: null, threadId: "native-thread", finishedAt: 1792720800 } },
};
export const runtimeUiTranscripts: Record<string, TranscriptEvent[]> = { planner: [
 { turnId: "planner", sequence: 1, event: { kind: "thread", id: "hidden-wire-thread" } },
 { turnId: "planner", sequence: 2, event: { kind: "text", text: "# An" } },
 { turnId: "planner", sequence: 3, event: { kind: "text", text: "swer\n\n**Hello**" } },
 { turnId: "planner", sequence: 4, event: { kind: "done", inputTokens: 11, outputTokens: 22, costUsd: 0.02, premiumRequests: null } },
] };
