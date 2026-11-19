import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { runtimeContextAgents, runtimeContextInputRequest, type ResolvedContextInputs } from "../../lib/contextInputs";
import { runtimeTaskContext, type RuntimeSnapshot, type RuntimeTurn } from "../../lib/runtime";
import { useContextInputs } from "../../lib/useContextInputs";
import { useModelLabel } from "../../lib/useModelLabel";
import { useRuntime } from "../../stores/runtime";
import { ContextDecisionDetails } from "../workspace/ContextDecisionDetails";
import { ContextUsageRow } from "../workspace/ContextUsagePopover";

export function RuntimeContextUsage({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const run = useRuntime(state => state.runs[sessionId]);
  if (!run) return null;

  const order = run.turnOrder ?? [];
  const agents = runtimeContextAgents(run.snapshot, order);
  if (agents.size === 0) return <p className="text-muted">{t("workspace.context.unavailableInput")}</p>;

  return Array.from(agents, ([id, turn]) => {
    const taskId = turn.taskId;
    const useTaskPreview = !!taskId && turn.role === "executor" && !!turn.threadId &&
      turn.ended && turn.cleanupConfirmed && run.snapshot.execution?.tasks[taskId]?.attempts.at(-1) === turn.id;
    const request = useTaskPreview
      ? null
      : runtimeContextInputRequest(run.snapshot, run.transcripts, order, turn);
    return (
      <RuntimeAgentUsage key={id} snapshot={run.snapshot} turn={turn} request={request} useTaskPreview={useTaskPreview} />
    );
  });
}

function RuntimeAgentUsage({
  snapshot,
  turn,
  request,
  useTaskPreview,
}: {
  snapshot: RuntimeSnapshot;
  turn: RuntimeTurn;
  request: ReturnType<typeof runtimeContextInputRequest>;
  useTaskPreview: boolean;
}) {
  const { t } = useTranslation();
  const modelLabel = useModelLabel();
  const taskId = turn.taskId;
  const taskTitle = snapshot.plan?.plan.tasks.find(task => task.id === taskId)?.title;
  const role = t(`sessionHistory.roles.${turn.role.toUpperCase()}`, { defaultValue: turn.role });
  const label = taskId ? [taskId, taskTitle, role].filter(Boolean).join(" · ") : role;
  const model = modelLabel({
    provider: turn.observedProvider ?? turn.requested.provider,
    requestedModel: turn.requested.requestedModel,
    modelName: turn.observedModel ?? undefined,
  });
  const preview = useContextInputs(request, request != null);
  const source = useTaskPreview ? turn.id : null;
  const [resolved, setResolved] = useState<{ source: string; inputs: ResolvedContextInputs }>();
  const runId = snapshot.runId;
  useEffect(() => {
    if (!source || !taskId) return;
    let current = true;
    void runtimeTaskContext(runId, taskId, source).then(inputs => {
      if (current) setResolved({ source, inputs });
    }).catch(() => {
      if (current) setResolved(undefined);
    });
    return () => { current = false; };
  }, [runId, taskId, source]);

  const inputs = source && resolved?.source === source ? resolved.inputs : preview;
  return (
    <>
      <ContextUsageRow label={label} model={model} inputs={inputs} />
      {!taskId && snapshot.config.contextPolicy !== "off" && <ContextDecisionDetails inputs={preview} />}
    </>
  );
}
