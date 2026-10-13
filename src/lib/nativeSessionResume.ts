import type { TurnMode } from "./ipc";
import { sameRef, type ModelRef } from "./models";

export interface NativeThread {
  ref: ModelRef;
  id: string;
  fork?: boolean;
  mode?: TurnMode;
}

/** Cursor persists ask mode in native sessions; switching modes needs a fresh
 * session with Tokage history, otherwise an executor still cannot edit files. */
export function canResumeNativeThread(thread: NativeThread | null, ref: ModelRef, mode: TurnMode): boolean {
  if (!thread || !sameRef(thread.ref, ref)) return false;
  return ref.provider !== "CURSOR_CLI" || thread.mode === mode;
}
