import { useMemo } from "react";
import { SessionHistoryView, type SessionHistorySource } from "../workspace/SessionHistoryView";
import { useProjects } from "../../stores/projects";
import { useRuntime } from "../../stores/runtime";
import { listAttempts, getChatLogs } from "../../lib/ipc";
import { runtimeList } from "../../lib/runtime";
import { runtimeHistoryAttempts, runtimeHistoryLogs } from "../../lib/runtimeHistory";

export function RuntimeHistoryView({ sessionId }: { sessionId: string }) {
  const project = useProjects(s => s.projects.find(p => p.sessions.some(session => session.id === sessionId)));
  const source = useMemo<SessionHistorySource>(() => ({
    listAttempts: async scope => {
      const runIds = scope === "session" ? [sessionId] : (await runtimeList()).filter(run => run.runId === sessionId || project?.sessions.some(session => session.id === run.runId)).map(run => run.runId);
      const rows = scope === "project" && project ? await listAttempts(undefined, project.id) : [];
      for (const id of runIds) {
        await useRuntime.getState().refresh(id);
        const projection = useRuntime.getState().runs[id];
        if (projection) rows.push(...runtimeHistoryAttempts(projection.snapshot, projection.transcripts));
      }
      return rows;
    },
    getChatLogs: async turnId => {
      const owner = Object.values(useRuntime.getState().runs).find(run => run.snapshot.turns?.[turnId]);
      if (!owner) return getChatLogs(turnId);
      await useRuntime.getState().refresh(owner.snapshot.runId);
      const current = useRuntime.getState().runs[owner.snapshot.runId];
      return runtimeHistoryLogs(current.snapshot, turnId, current.transcripts[turnId] ?? []);
    },
  }), [sessionId, project?.id, project?.sessions.length]);
  return <SessionHistoryView sessionId={sessionId} projectId={project?.id} source={source} />;
}
