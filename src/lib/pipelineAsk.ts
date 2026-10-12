/**
 * Independent question during plan execution: answered by the planning model in a fresh,
 * read-only (plan-mode) CLI session that is given a snapshot of the pipeline. It never
 * touches the session's own CLI thread or any task's session.
 */

import { useChat } from "../stores/chat";
import { useExecution } from "../stores/execution";
import { sessionCwd, useProjects } from "../stores/projects";
import { planTurn } from "./turn";

const REPORT_CHARS = 600;

export function pipelineSnapshot(sessionId: string): string {
  const pipeline = useExecution.getState().pipelines[sessionId];
  if (!pipeline) return "";
  const lines = pipeline.tasks.map((t) => {
    const state = pipeline.taskStates[t.id] ?? "PENDING";
    const report = pipeline.upstreamArtifacts?.[t.id]?.report?.trim();
    const files = pipeline.upstreamArtifacts?.[t.id]?.changedFiles ?? [];
    return [
      `- ${t.id} [${state}] ${t.title}`,
      `  goal: ${t.goal}`,
      files.length ? `  changed files: ${files.slice(0, 12).join(", ")}` : "",
      report ? `  report: ${report.slice(0, REPORT_CHARS)}` : "",
    ]
      .filter(Boolean)
      .join("\n");
  });
  return lines.join("\n");
}

/** Returns false when no planning model is available. */
export async function askAboutPipeline(sessionId: string, question: string): Promise<boolean> {
  const project = useProjects.getState().projects.find((p) => p.sessions.some((s) => s.id === sessionId));
  if (!project) return false;
  const turn = planTurn(question, "plan");
  if (!turn) return false;
  const promptPrefix = [
    "This is a standalone question about a plan that is being executed by other agents.",
    "Answer from the snapshot below and by reading the project. Do not modify any files and",
    "do not run commands that change anything; the running tasks own the working tree.",
    "",
    "## Execution snapshot",
    pipelineSnapshot(sessionId),
  ].join("\n");
  await useChat.getState().send({
    sessionId,
    cwd: sessionCwd(project, sessionId),
    text: question,
    ...turn,
    framePlanningPrompt: false,
    promptPrefix,
    isolated: true,
  });
  return true;
}
