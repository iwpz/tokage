/**
 * F16.c: modal showing incremental plan diff before applying a replan.
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { PlanDiff } from "../../lib/ipc";
import { Button } from "../ui";

interface PlanDiffViewProps {
  diff: PlanDiff;
  onConfirm: () => void | Promise<void>;
  onCancel: () => void;
  busy?: boolean;
}

export function PlanDiffView({ diff, onConfirm, onCancel, busy }: PlanDiffViewProps) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);

  const taskChanges = diff.tasks.filter((x) => x.action !== "unchanged");
  const contractChanges = diff.contracts.filter((x) => x.action !== "unchanged");

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t("plan.diff.title", "计划差异")}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    >
      <div className="flex max-h-[80vh] w-full max-w-2xl flex-col rounded-xl border border-border bg-elevated shadow-xl">
        <header className="border-b border-border px-5 py-3">
          <h2 className="text-sm font-semibold text-fg">
            {t("plan.diff.title", "计划差异")}{" "}
            <span className="font-mono text-xs text-muted">
              v{diff.fromVersion} → v{diff.toVersion}
            </span>
          </h2>
          <p className="mt-0.5 text-xs text-muted">
            {t("plan.diff.subtitle", "确认后将应用增量重规划并生成适配任务。")}
          </p>
        </header>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-5 py-4 text-sm">
          {diff.specDiff.goalChanged && (
            <section>
              <h3 className="mb-1 text-xs font-medium text-muted">Spec · 目标</h3>
              <p className="rounded-md bg-danger/10 px-2 py-1 text-xs line-through opacity-70">
                {diff.specDiff.oldGoal || "—"}
              </p>
              <p className="mt-1 rounded-md bg-success/10 px-2 py-1 text-xs">
                {diff.specDiff.newGoal || "—"}
              </p>
            </section>
          )}

          {contractChanges.length > 0 && (
            <section>
              <h3 className="mb-1 text-xs font-medium text-muted">
                Contracts ({contractChanges.length})
              </h3>
              <ul className="space-y-1">
                {contractChanges.map((c) => (
                  <li key={c.id} className="rounded-md border border-border px-2 py-1.5 text-xs">
                    <span className="font-mono font-semibold">{c.id}</span>{" "}
                    <span className="text-muted">{c.title}</span>
                    <span className="ml-2 uppercase text-[10px] text-accent">{c.action}</span>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {taskChanges.length > 0 && (
            <section>
              <h3 className="mb-1 text-xs font-medium text-muted">
                Tasks ({taskChanges.length})
              </h3>
              <ul className="space-y-1">
                {taskChanges.map((tk) => (
                  <li key={tk.id} className="rounded-md border border-border px-2 py-1.5 text-xs">
                    <span className="font-mono font-semibold">{tk.id}</span>{" "}
                    <span className="text-muted">{tk.title}</span>
                    <span className="ml-2 uppercase text-[10px] text-accent">{tk.action}</span>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {diff.notesChanged && (
            <p className="text-xs text-muted">Notes 有变更。</p>
          )}

          {!diff.specDiff.goalChanged &&
            contractChanges.length === 0 &&
            taskChanges.length === 0 &&
            !diff.notesChanged && (
              <p className="text-xs text-muted">无实质差异。</p>
            )}
        </div>

        <footer className="flex justify-end gap-2 border-t border-border px-5 py-3">
          <Button variant="ghost" onClick={onCancel} disabled={confirming || busy}>
            {t("common.cancel", "取消")}
          </Button>
          <Button
            variant="primary"
            disabled={confirming || busy}
            onClick={() => {
              setConfirming(true);
              void Promise.resolve(onConfirm()).finally(() => setConfirming(false));
            }}
          >
            {confirming || busy
              ? t("plan.diff.applying", "应用中…")
              : t("plan.diff.apply", "确认应用")}
          </Button>
        </footer>
      </div>
    </div>
  );
}
