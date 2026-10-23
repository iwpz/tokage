// Offline component transport only. Never imported by production entrypoints.
import { runtimeUiSnapshot, runtimeUiTranscripts } from "../fixtures/runtime-ui";
export const snapshot = structuredClone(runtimeUiSnapshot);
snapshot.config.models.executor.userLocked = false;
snapshot.config.models.alternative = { ...snapshot.config.models.executor, provider: "CODEX_CLI", requestedModel: "gpt-6-luna" };
export const runtimeSnapshot = async () => snapshot;
export const runtimeEvents = async () => [];
export const runtimeTranscript = async (_run: string, turn: string, after: number) => (runtimeUiTranscripts[turn] ?? []).filter(row => row.sequence > after);
export const runtimeWorkerError = async () => null;
export const runtimeList = async () => [snapshot];
export const runtimeInspectTask = async () => { throw new Error("Offline component fixture cannot inspect a real workspace"); };
export const runtimeSubmit = async () => { throw new Error("Offline component fixture cannot dispatch"); };
export const runtimeRequest = () => { throw new Error("Offline component fixture cannot dispatch"); };
