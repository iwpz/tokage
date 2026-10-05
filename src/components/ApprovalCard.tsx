import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy, Loader2, ShieldAlert, ShieldCheck, ShieldX, Terminal } from "lucide-react";
import { cn } from "../lib/cn";
import type { UserDecision } from "../lib/ipc";
import { useChat, type ApprovalPart, type ChatMessage } from "../stores/chat";
import { Button } from "./ui";

interface ApprovalCardProps {
  p: ApprovalPart;
  m: ChatMessage;
  sessionId: string;
}

export function ApprovalCard({ p, m: _m, sessionId }: ApprovalCardProps) {
  const { t } = useTranslation();
  const resolveApproval = useChat((s) => s.resolveApproval);
  const [submitting, setSubmitting] = useState(false);
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(p.command);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Ignore clipboard write failures
    }
  };

  const handleDecision = async (decision: UserDecision) => {
    if (submitting) return;
    setSubmitting(true);
    try {
      await resolveApproval(sessionId, p.id, decision);
    } finally {
      setSubmitting(false);
    }
  };

  if (p.resolved) {
    const isDeny = p.decision === "deny";
    const decisionKey =
      p.decision === "allow_once"
        ? "allowedOnce"
        : p.decision === "allow_session"
          ? "allowedSession"
          : p.decision === "allow_project"
            ? "allowedProject"
            : "denied";

    return (
      <div className="my-2 flex items-center gap-2 rounded-lg border border-border bg-hover/50 px-3 py-2 text-xs">
        {isDeny ? (
          <ShieldX aria-hidden className="size-4 shrink-0 text-danger" />
        ) : (
          <ShieldCheck aria-hidden className="size-4 shrink-0 text-success" />
        )}
        <span className={cn("font-medium", isDeny ? "text-danger" : "text-success")}>
          {t(`workspace.chat.approval.${decisionKey}`)}:
        </span>
        <code className="min-w-0 flex-1 truncate font-mono text-fg" title={p.command}>
          {p.command}
        </code>
      </div>
    );
  }

  const reasonText =
    p.reason === "sandbox_denied"
      ? t("workspace.chat.approval.reasonSandbox")
      : t("workspace.chat.approval.reasonDisallowed");

  return (
    <div
      role="alert"
      aria-labelledby={`approval-title-${p.id}`}
      className="my-3 rounded-xl border border-warning/40 bg-elevated p-4 shadow-sm"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-center gap-2">
          <div className="flex size-7 items-center justify-center rounded-lg bg-warning/15 text-warning">
            <ShieldAlert aria-hidden className="size-4" />
          </div>
          <div>
            <h3 id={`approval-title-${p.id}`} className="text-sm font-semibold text-fg">
              {t("workspace.chat.approval.title")}
            </h3>
            <p className="text-xs text-subtle">{t("workspace.chat.approval.badge")}</p>
          </div>
        </div>
        <span className="rounded-full bg-warning/15 px-2.5 py-0.5 text-[11px] font-medium text-warning">
          {t("workspace.chat.approval.sensitive")}
        </span>
      </div>

      <div className="mt-3 text-xs leading-relaxed text-muted">
        {reasonText}
      </div>

      {/* Command Preview Box */}
      <div className="relative mt-2.5 rounded-lg border border-border bg-bg/80 p-3 font-mono text-xs">
        <div className="flex items-start justify-between gap-2">
          <div className="flex items-center gap-2 overflow-x-auto pr-8 text-fg">
            <Terminal aria-hidden className="size-3.5 shrink-0 text-subtle" />
            <span className="whitespace-pre-wrap break-all select-all font-mono">{p.command}</span>
          </div>
          <button
            type="button"
            onClick={handleCopy}
            title={copied ? t("workspace.chat.copied") : t("workspace.chat.copy")}
            className="absolute top-2.5 right-2.5 flex size-6 items-center justify-center rounded text-muted hover:bg-hover hover:text-fg"
          >
            {copied ? <Check aria-hidden className="size-3.5 text-success" /> : <Copy aria-hidden className="size-3.5" />}
          </button>
        </div>
      </div>

      <p className="mt-2 text-[11px] text-subtle">{t("workspace.chat.approval.hint")}</p>

      {/* Actions */}
      <div className="mt-4 flex flex-wrap items-center gap-2">
        <Button
          variant="primary"
          size="sm"
          disabled={submitting}
          onClick={() => handleDecision("allow_session")}
          className="gap-1.5"
        >
          {submitting ? <Loader2 aria-hidden className="size-3.5 animate-spin" /> : null}
          {t("workspace.chat.approval.allowSession")}
        </Button>

        <Button
          variant="secondary"
          size="sm"
          disabled={submitting}
          onClick={() => handleDecision("allow_project")}
        >
          {t("workspace.chat.approval.allowProject")}
        </Button>

        <Button
          variant="secondary"
          size="sm"
          disabled={submitting}
          onClick={() => handleDecision("allow_once")}
        >
          {t("workspace.chat.approval.allowOnce")}
        </Button>

        <Button
          variant="ghost"
          size="sm"
          disabled={submitting}
          onClick={() => handleDecision("deny")}
          className="ml-auto text-danger hover:bg-danger/10 hover:text-danger"
        >
          {t("workspace.chat.approval.deny")}
        </Button>
      </div>
    </div>
  );
}
