import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, CheckCircle2, Clock, GitBranch, Loader2, ShieldAlert } from "lucide-react";
import { cn } from "../lib/cn";
import { createSessionWorktree } from "../lib/ipc";
import { useProjects } from "../stores/projects";
import { useAuth, type FullAutoTtl } from "../stores/auth";
import { Button } from "./ui";

interface FullAutoConfirmDialogProps {
  sessionId: string;
  onClose: () => void;
  onConfirmed?: () => void;
}

export function FullAutoConfirmDialog({ sessionId, onClose, onConfirmed }: FullAutoConfirmDialogProps) {
  const { t } = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const [ttl, setTtl] = useState<FullAutoTtl>("1h");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const project = useProjects((s) => s.projects.find((p) => p.sessions.some((x) => x.id === sessionId)));
  const session = project?.sessions.find((x) => x.id === sessionId);

  useEffect(() => {
    dialog.current?.showModal();
  }, []);

  const handleConfirm = async () => {
    if (!project || !session) return;
    setBusy(true);
    setError(null);
    try {
      if (!session.worktree) {
        const wt = await createSessionWorktree(project.path, `s-${Date.now().toString(36)}`);
        useProjects.getState().setSessionWorktree(sessionId, wt);
      }
      useAuth.getState().grantFullAuto(sessionId, ttl);
      onConfirmed?.();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const ttlOptions: { id: FullAutoTtl; label: string; desc: string }[] = [
    {
      id: "1h",
      label: t("settings.security.fullAutoConfirm.ttl1h"),
      desc: t("settings.security.fullAutoConfirm.ttl1hDesc"),
    },
    {
      id: "today",
      label: t("settings.security.fullAutoConfirm.ttlToday"),
      desc: t("settings.security.fullAutoConfirm.ttlTodayDesc"),
    },
    {
      id: "session",
      label: t("settings.security.fullAutoConfirm.ttlSession"),
      desc: t("settings.security.fullAutoConfirm.ttlSessionDesc"),
    },
  ];

  return (
    <dialog
      ref={dialog}
      aria-labelledby="fullauto-confirm-title"
      aria-describedby="fullauto-confirm-desc"
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
      className="m-auto w-[32rem] max-w-[calc(100vw-2rem)] rounded-xl border border-border bg-elevated p-0 text-fg shadow-2xl backdrop:bg-black/50"
    >
      <div className="space-y-4 px-6 pt-5 pb-4">
        <div className="flex items-center gap-2.5 text-warning">
          <ShieldAlert className="size-5 shrink-0" />
          <h2 id="fullauto-confirm-title" className="text-[15px] font-semibold text-fg">
            {t("settings.security.fullAutoConfirm.title")}
          </h2>
        </div>

        <div className="rounded-lg border border-warning/30 bg-warning-soft/30 p-3 text-xs leading-relaxed text-muted">
          <p id="fullauto-confirm-desc" className="text-fg font-medium mb-1">
            {t("settings.security.fullAutoConfirm.warningHeader")}
          </p>
          <p>{t("settings.security.fullAutoConfirm.warningBody")}</p>
        </div>

        <div className="space-y-2">
          <h3 className="text-xs font-medium text-subtle uppercase tracking-wider">
            {t("settings.security.fullAutoConfirm.protectionsTitle")}
          </h3>
          <ul className="space-y-1.5 text-xs text-muted">
            <li className="flex items-start gap-2">
              <CheckCircle2 className="size-3.5 text-success shrink-0 mt-0.5" />
              <span>{t("settings.security.fullAutoConfirm.protectionSandbox")}</span>
            </li>
            <li className="flex items-start gap-2">
              <CheckCircle2 className="size-3.5 text-success shrink-0 mt-0.5" />
              <span>{t("settings.security.fullAutoConfirm.protectionRedlines")}</span>
            </li>
            <li className="flex items-start gap-2">
              <CheckCircle2 className="size-3.5 text-success shrink-0 mt-0.5" />
              <span>{t("settings.security.fullAutoConfirm.protectionSnapshot")}</span>
            </li>
            <li className="flex items-start gap-2">
              <CheckCircle2 className="size-3.5 text-success shrink-0 mt-0.5" />
              <span>{t("settings.security.fullAutoConfirm.protectionWorktree")}</span>
            </li>
          </ul>
        </div>

        {!session?.worktree && (
          <div className="flex items-center gap-2 rounded-md bg-hover px-3 py-2 text-xs text-muted">
            <GitBranch className="size-4 shrink-0 text-accent" />
            <span>{t("settings.security.fullAutoConfirm.worktreeNotice")}</span>
          </div>
        )}

        <div className="space-y-2">
          <label className="text-xs font-medium text-subtle uppercase tracking-wider flex items-center gap-1.5">
            <Clock className="size-3.5" />
            {t("settings.security.fullAutoConfirm.ttlTitle")}
          </label>
          <div className="grid grid-cols-3 gap-2">
            {ttlOptions.map((opt) => (
              <button
                key={opt.id}
                type="button"
                onClick={() => setTtl(opt.id)}
                className={cn(
                  "flex flex-col items-start p-2 rounded-lg border text-left transition-colors",
                  ttl === opt.id
                    ? "border-accent bg-accent/10 text-fg"
                    : "border-border hover:bg-hover text-muted",
                )}
              >
                <span className="text-xs font-medium">{opt.label}</span>
                <span className="text-[11px] text-subtle mt-0.5">{opt.desc}</span>
              </button>
            ))}
          </div>
        </div>

        {error && (
          <div role="alert" className="flex items-start gap-2 text-xs text-danger">
            <AlertTriangle className="size-4 shrink-0 mt-0.5" />
            <span className="break-words">{error}</span>
          </div>
        )}
      </div>

      <div className="flex items-center justify-end gap-2.5 border-t border-border px-6 py-3 bg-base/40">
        <Button onClick={onClose} disabled={busy}>
          {t("settings.security.fullAutoConfirm.cancel")}
        </Button>
        <Button variant="primary" className="bg-warning text-black hover:bg-warning/90 font-medium" onClick={handleConfirm} disabled={busy}>
          {busy ? (
            <>
              <Loader2 className="size-3.5 animate-spin" />
              {t("settings.security.fullAutoConfirm.enabling")}
            </>
          ) : (
            t("settings.security.fullAutoConfirm.confirm")
          )}
        </Button>
      </div>
    </dialog>
  );
}
