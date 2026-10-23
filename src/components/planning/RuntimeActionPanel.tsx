import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { RuntimeSnapshot } from "../../lib/runtime";
import { Badge } from "../ui";

/** Shared Plan/Pipeline footer, following PlanReview's Bottom Floating Actions bar. */
export function RuntimeActionPanel({ snapshot, children, error }: {
  snapshot: RuntimeSnapshot; children: ReactNode; error?: string | null;
}) {
  const { t } = useTranslation();
  const plan = snapshot.plan?.plan;
  return <footer data-testid="runtime-action-panel" aria-label={t("runtime.title")} className="mx-4 mb-4 mt-2.5 shrink-0 rounded-xl border border-border bg-elevated px-5 py-2.5 shadow-sm">
    {error && <p role="alert" className="mb-2 break-words text-xs text-danger">{error}</p>}
    <div className="flex min-w-0 flex-wrap items-center justify-between gap-x-5 gap-y-2.5">
      <div className="flex min-w-0 flex-wrap items-center gap-3 text-xs text-muted">
        {plan && <div className="flex flex-wrap items-center gap-1.5"><span className="font-semibold text-fg">{t("plan.review.summaryTasks", { count: plan.tasks.length })}</span><span>·</span><span className="font-semibold text-fg">{t("plan.review.summaryContracts", { count: plan.contracts.length })}</span></div>}
        <Badge size="xs" variant={snapshot.state === "Blocked" ? "warning" : snapshot.state === "Executing" ? "accent" : "muted"}>{t(`runtime.states.${snapshot.state}`, { defaultValue: snapshot.state })}</Badge>
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-2.5">{children}</div>
    </div>
  </footer>;
}
