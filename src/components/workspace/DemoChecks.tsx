import { useTranslation } from "react-i18next";
import { useDemoAudit } from "../../lib/demoAudit";
import { useExecution } from "../../stores/execution";
import { demoIdentity } from "../../lib/demoIdentity";

export function DemoChecks({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const turns = useDemoAudit(s => s.turns);
  const pipeline = useExecution(s => s.pipelines[sessionId]);
  if (!demoIdentity(sessionId)) return null;
  const records = Object.values(turns).filter(turn => turn.sessionId === sessionId);
  const status = (value: boolean | undefined) => t(value === undefined ? "demo.pending" : value ? "demo.passed" : "demo.failed");
  const usage = records.filter(turn => turn.inputTokens !== undefined);
  const route = records.some(turn => turn.route && turn.route !== "DEBUG_PROVIDER/mock") ? false : records.length && records.every(turn => turn.route) ? true : undefined;
  const zero = usage.length ? usage.every(turn => turn.inputTokens === 0 && turn.outputTokens === 0 && turn.apiCalls === 0 && turn.costUsd === 0) : undefined;
  const once = records.some(turn => turn.ended > 1) ? false : records.length && records.every(turn => turn.ended === 1) ? true : undefined;
  const started = pipeline?.tasks.filter(task => pipeline.taskStartedAt?.[task.id] !== undefined) ?? [];
  const dependencies = started.length ? started.every(task => task.depends_on.every(dep => {
    const end = pipeline?.taskEndedAt?.[dep];
    const start = pipeline?.taskStartedAt?.[task.id];
    return end !== undefined && start !== undefined && end <= start;
  })) : undefined;
  const done = pipeline?.tasks.filter(task => pipeline.taskStates[task.id] === "DONE").length ?? 0;
  return <details className="border-b border-border px-4 py-2 text-xs">
    <summary className="cursor-pointer text-muted">{t("demo.checks")} · {done}/{pipeline?.tasks.length ?? 7}</summary>
    <ul className="mt-2 grid gap-1 text-subtle sm:grid-cols-2">
      <li>{t("demo.checkRoute")}: {status(route)}</li>
      <li>{t("demo.checkUsage")}: {status(zero)}</li>
      <li>{t("demo.checkEnded")}: {status(once)}</li>
      <li>{t("demo.checkDependencies")}: {status(dependencies)}</li>
    </ul>
    <p className="mt-2 text-subtle">{t("demo.checkNote")}</p>
  </details>;
}
