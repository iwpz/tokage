import type { ActivePipelineSession } from "../stores/execution";
import type { TaskItem } from "./ipc";
import {
  type WindowReuseDecision,
} from "./ipc";

export interface WindowReuseCandidate {
  taskId: string;
  threadId: string;
  cwd: string;
  decision: WindowReuseDecision;
}

/** The retired frontend scheduler has no frozen model/usage provenance.
 * Keep fresh execution until a shared runtime owns the window; do not invent prices or occupancy. */
export async function findReusableWindow(options: {
  pipeline: ActivePipelineSession;
  task: TaskItem;
  model: string;
  prompt: string;
  /** Prefer same cwd; when set, only candidates with this cwd are considered. */
  preferredCwd?: string | null;
}): Promise<WindowReuseCandidate | null> {
  void options;
  return null;
}
