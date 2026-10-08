/**
 * Minimal production UiEvent channel (docs/07 §4).
 * Batches for ~50ms then fans out to subscribers — mirrors `UiEventBatcher`.
 */

export type TaskState =
  | "PENDING"
  | "WAITING_UPSTREAM"
  | "READY"
  | "RUNNING"
  | "VERIFYING"
  | "REPAIRING"
  | "DONE"
  | "BLOCKED"
  | "CANCELLED"
  | "SKIPPED"
  | "INTERRUPTED";

export type UiEvent =
  | { type: "tree_changed"; sessionId: string }
  | {
      type: "task_state_changed";
      taskId: string;
      state: TaskState;
      attemptId?: string | null;
    }
  | {
      type: "main_session_status_changed";
      sessionId: string;
      status: string;
    }
  | {
      type: "gate_completed";
      taskId: string;
      passed: boolean;
      summary: string;
    }
  | {
      type: "approval_required";
      item: {
        approvalId: string;
        taskId: string;
        attemptId: string;
        command: string;
        reason: string;
        createdAtMs: number;
      };
    }
  | { type: "notice"; level: string; message: string };

type Listener = (batch: UiEvent[]) => void;

const listeners = new Set<Listener>();
let buffer: UiEvent[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;
const FLUSH_MS = 50;

function flush(): void {
  flushTimer = null;
  if (buffer.length === 0) return;
  const batch = buffer;
  buffer = [];
  for (const cb of listeners) {
    try {
      cb(batch);
    } catch (err) {
      console.error("[uiEvents] listener error:", err);
    }
  }
}

/** Push a UI event; drained to subscribers after ≤50ms. */
export function pushUiEvent(event: UiEvent): void {
  buffer.push(event);
  if (flushTimer == null) {
    flushTimer = setTimeout(flush, FLUSH_MS);
  }
}

/** Subscribe to batched UiEvents. Returns unsubscribe. */
export function subscribeUiEvents(cb: Listener): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Test helper: drain immediately. */
export function drainUiEventsNow(): UiEvent[] {
  if (flushTimer != null) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  const batch = buffer;
  buffer = [];
  return batch;
}
