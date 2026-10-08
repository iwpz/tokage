import type { ActivePipelineSession, TaskApproval } from "../stores/execution";

/** Mirrors `tokage_orchestration::tree::UnifiedApprovalItem` (docs/07 §5). */
export interface UnifiedApprovalItem {
  approvalId: string;
  taskId: string;
  attemptId: string;
  command: string;
  reason: string;
  createdAtMs: number;
  level: string;
  /** Original board item for resolve wiring. */
  source: TaskApproval;
}

/** Flatten unresolved per-task approvals into a session-wide queue (oldest first). */
export function collectUnifiedApprovals(
  pipeline: ActivePipelineSession | null | undefined,
): UnifiedApprovalItem[] {
  if (!pipeline?.taskApprovals) return [];
  const items: UnifiedApprovalItem[] = [];
  for (const [taskId, list] of Object.entries(pipeline.taskApprovals)) {
    for (const a of list) {
      if (a.resolved) continue;
      items.push({
        approvalId: a.id,
        taskId,
        attemptId: a.turnId,
        command: a.command,
        reason: a.reason,
        createdAtMs: a.createdAtMs ?? 0,
        level: a.level,
        source: a,
      });
    }
  }
  items.sort((x, y) => x.createdAtMs - y.createdAtMs);
  return items;
}
