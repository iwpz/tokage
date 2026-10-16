import { gitRollbackChanges, revertOutOfScopeFiles } from "./ipc";
import { checkpointPipeline, useExecution } from "../stores/execution";
import { withRollbackCheckout } from "./scopeGuard";

/** Cancellation/failure and scope violation use the same immutable native turn evidence. */
export async function restoreTaskChanges(sessionId: string, taskId: string, all: boolean,
  confirm: (files: string[]) => boolean): Promise<string | null> {
  const pipeline = useExecution.getState().pipelines[sessionId];
  const evidence = pipeline?.taskFailureEvidence?.[taskId];
  const latestTurnId = pipeline?.taskTurnIds?.[taskId];
  const turnId = all ? latestTurnId : evidence?.turnId;
  const workdir = all ? pipeline?.taskCwds?.[taskId] || pipeline?.cwd : evidence?.workdir;
  if (!turnId || !workdir) throw new Error("Missing turn identity or execution directory; manual recovery required");
  if (["RUNNING", "VERIFYING", "REPAIRING", "DONE"].includes(pipeline.taskStates[taskId]) || pipeline.gitDeliveries?.[taskId]) {
    throw new Error("Task is active or has accepted Git delivery; rollback is unavailable");
  }
  return withRollbackCheckout(workdir, async () => {
    const files = all ? await gitRollbackChanges(workdir, turnId) : evidence?.outOfScopeFiles ?? [];
    if (files.length === 0 || !confirm(files)) return null;
    const live = useExecution.getState().pipelines[sessionId];
    if (live?.taskTurnIds?.[taskId] !== latestTurnId || live.taskStates[taskId] !== pipeline.taskStates[taskId]) throw new Error("Task changed while reviewing rollback");
    const summary = await revertOutOfScopeFiles(workdir, files, turnId);
    if (evidence?.turnId === turnId) useExecution.getState().recordTaskFailureEvidence(sessionId, taskId, { ...evidence, outOfScopeFiles: [] });
    await checkpointPipeline(sessionId);
    return summary;
  });
}
