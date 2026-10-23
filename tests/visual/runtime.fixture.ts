// Offline component transport only. Never imported by production entrypoints.
import { runtimeUiSnapshot, runtimeUiTranscripts } from "../fixtures/runtime-ui";
export const snapshot = structuredClone(runtimeUiSnapshot);
snapshot.config.models.executor.userLocked = false;
snapshot.config.models.alternative = { ...snapshot.config.models.executor, provider: "CODEX_CLI", requestedModel: "gpt-6-luna" };
// Explicit offline state matrix; no product state or authorization is modified.
const state = new URLSearchParams(location.search).get("state");
if (state && ["Approved", "Executing", "Blocked", "AwaitingMerge", "Stopped"].includes(state)) {
 snapshot.state = state;
 snapshot.approvedDigest = "offline-digest";
 snapshot.execution = { sequence: 1, tasks: { T1: { state: state === "Blocked" ? "BLOCKED" : state === "AwaitingMerge" ? "DONE" : state === "Approved" || state === "Stopped" ? "PENDING" : "RUNNING", assignment: snapshot.config.models.executor, attempts: [], error: null, deviation: false, repairStep: "none" } } };
 if (state === "AwaitingMerge") snapshot.delivery = { candidate: { head: "offline-candidate" }, confirmationDigest: "offline-delivery" };
}
export const runtimeSnapshot = async () => snapshot;
export const runtimeEvents = async () => [];
export const runtimeTranscript = async (_run: string, turn: string, after: number) => (runtimeUiTranscripts[turn] ?? []).filter(row => row.sequence > after);
export const runtimeWorkerError = async () => null;
export const runtimeList = async () => [snapshot];
export const runtimeInspectTask = async () => { throw new Error("Offline component fixture cannot inspect a real workspace"); };
export const runtimeSubmit = async () => { throw new Error("Offline component fixture cannot dispatch"); };
export const runtimeRequest = () => { throw new Error("Offline component fixture cannot dispatch"); };
