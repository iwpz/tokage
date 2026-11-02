import { contextHistorySourceIds } from "../../lib/contextActionHistory";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight, History, RotateCw } from "lucide-react";
import { listContextActionHistory, type ContextActionHistoryRow } from "../../lib/ipc";
import { useChat } from "../../stores/chat";
import { Button } from "../ui";

export function ContextActionHistory({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const conv = useChat((s) => s.conversations[sessionId]);
  const [open, setOpen] = useState(false);
  const [revision, refresh] = useState(0);
  const [rows, setRows] = useState<ContextActionHistoryRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const ids = contextHistorySourceIds(conv, sessionId);
  const idsKey = JSON.stringify(ids);
  const receiptsKey = JSON.stringify([
    conv?.pendingCompact?.actionId,
    conv?.messages
      .filter((m) => m.compactAction)
      .map((m) => [m.id, m.compactAction?.state, m.compactAction?.failureReceipt]),
  ]);

  useEffect(() => {
    if (!open) return;
    let current = true;
    setLoading(true);
    setError("");
    setRows([]);
    void listContextActionHistory(sessionId, JSON.parse(idsKey))
      .then((result) => {
        if (current) setRows(result);
      })
      .catch((e) => {
        if (current) setError(String(e));
      })
      .finally(() => {
        if (current) setLoading(false);
      });
    return () => {
      current = false;
    };
  }, [open, sessionId, idsKey, receiptsKey, revision]);

  return (
    <details
      className="group rounded-xl surface-card border border-line p-3 text-xs transition-colors"
      open={open}
      onToggle={(e) => setOpen(e.currentTarget.open)}
    >
      <summary className="flex cursor-pointer list-none select-none items-center justify-between font-medium text-fg [&::-webkit-details-marker]:hidden">
        <span className="flex items-center gap-1.5">
          <History className="size-3.5 text-accent" />
          <span>{t("workspace.context.actionHistory")}</span>
        </span>
        <ChevronRight className="size-3.5 text-muted transition-transform duration-200 group-open:rotate-90" />
      </summary>
      {open && (
        <div className="mt-3 space-y-2.5 border-t border-line/60 pt-2.5">
          <p className="text-muted leading-relaxed">{t("workspace.context.historyHint")}</p>
          <div>
            <Button
              size="sm"
              variant="secondary"
              disabled={loading}
              onClick={() => refresh((n) => n + 1)}
              className="gap-1.5"
            >
              <RotateCw className={`size-3 ${loading ? "animate-spin" : ""}`} />
              {t("workspace.context.refreshHistory")}
            </Button>
          </div>
          {loading ? (
            <p role="status" className="text-muted py-1">
              {t("workspace.context.historyLoading")}
            </p>
          ) : error ? (
            <p role="alert" className="text-danger py-1">
              {error}
            </p>
          ) : rows.length === 0 ? (
            <p className="text-muted py-1">{t("workspace.context.historyEmpty")}</p>
          ) : (
            <ol className="space-y-2 pt-1">
              {rows.map((row) => (
                <li
                  key={row.id}
                  className="rounded-lg surface-secondary border border-border/40 p-2.5 space-y-1"
                >
                  <div className="flex items-center justify-between">
                    <span className="font-mono font-medium text-fg">#{row.id}</span>
                    <span className="rounded px-1.5 py-0.5 text-[11px] font-medium bg-hover text-muted">
                      {row.status}
                    </span>
                  </div>
                  <p className="text-muted">
                    {row.kind} · {row.trigger}
                    {row.method ? ` · ${row.method}` : ""}
                  </p>
                  <p className="break-all font-mono text-[11px] text-subtle">
                    {row.attemptId}
                    {row.newAttemptId ? ` → ${row.newAttemptId}` : ""}
                  </p>
                  {row.createdAt && <p className="text-[11px] text-subtle">{row.createdAt}</p>}
                  {row.actualSavingUsd != null && (
                    <p className="text-accent font-medium">
                      {t("workspace.context.historyEstimate", {
                        amount: row.actualSavingUsd.toFixed(4),
                      })}
                    </p>
                  )}
                </li>
              ))}
            </ol>
          )}
        </div>
      )}
    </details>
  );
}
