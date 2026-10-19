import type { TaskState } from "./ipc";

/** Allowed transitions (excluding self). Must match the gold file and Rust `TaskState::can_transition_to`. */
export const TASK_STATE_TRANSITIONS: Record<TaskState, readonly TaskState[]> = {
  PENDING: ["READY", "CANCELLED", "SKIPPED", "WAITING_UPSTREAM"],
  READY: ["RUNNING", "CANCELLED", "SKIPPED", "PAUSED_QUOTA", "WAITING_UPSTREAM", "BLOCKED"],
  RUNNING: ["VERIFYING", "REPAIRING", "BLOCKED", "CANCELLED", "PAUSED_QUOTA", "INTERRUPTED"],
  VERIFYING: ["DONE", "REPAIRING", "BLOCKED", "CANCELLED", "INTERRUPTED"],
  REPAIRING: ["RUNNING", "BLOCKED", "CANCELLED", "PAUSED_QUOTA", "INTERRUPTED"],
  BLOCKED: ["PENDING", "READY", "CANCELLED", "SKIPPED", "VERIFYING"],
  PAUSED_QUOTA: ["READY", "RUNNING", "CANCELLED"],
  WAITING_UPSTREAM: ["PENDING", "READY", "CANCELLED", "SKIPPED"],
  INTERRUPTED: ["READY", "RUNNING", "CANCELLED", "BLOCKED"],
  DONE: [],
  CANCELLED: [],
  SKIPPED: [],
};

export function canTransition(from: TaskState, to: TaskState): boolean {
  if (from === to) return true;
  return TASK_STATE_TRANSITIONS[from]?.includes(to) ?? false;
}
