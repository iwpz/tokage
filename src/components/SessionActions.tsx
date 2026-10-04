import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Archive, ArchiveRestore, Ellipsis, GitFork, Pencil, Trash2 } from "lucide-react";
import { cn } from "../lib/cn";
import { removeSessionWorktree } from "../lib/ipc";
import { usePopover } from "../lib/usePopover";
import { isStreaming, useChat } from "../stores/chat";
import { useProjects, type MainSession } from "../stores/projects";
import { Button, IconButton } from "./ui";

/** CLIs whose sessions Tokage can branch headlessly, keeping the prompt cache warm. */
const NATIVE_FORK = new Set(["CLAUDE_CODE"]);

/** Whether another session still works in the same worktree (forks share it). */
const worktreeShared = (session: MainSession) =>
  useProjects
    .getState()
    .projects.some((p) => p.sessions.some((x) => x.id !== session.id && x.worktree?.root === session.worktree?.root));

/** Forks a session: copies its conversation into a new one right below it, in the same folder. */
export function forkSession(session: MainSession, suffix: string) {
  const conv = useChat.getState().conversations[session.id];
  const id = useProjects.getState().forkSession(session.id, `${session.title}${suffix}`, session.worktree);
  if (!id) return;
  const native = !!conv?.thread && NATIVE_FORK.has(conv.thread.ref.provider);
  useChat.getState().forkConversation(session.id, id, native);
}

/**
 * "⋯" button on a sidebar session row: rename, fork, archive / restore, delete.
 * Rename edits the title in place through `onRename`; deleting asks first.
 */
export function SessionActions({ session, onRename }: { session: MainSession; onRename: () => void }) {
  const { t } = useTranslation();
  const { open, setOpen, ref } = usePopover();
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const [confirm, setConfirm] = useState<null | "delete" | "dirty">(null);
  const [error, setError] = useState<string | null>(null);
  const busy = useChat((s) => isStreaming(s.conversations[session.id]));
  const { setSessionArchived, deleteSession } = useProjects();

  useLayoutEffect(() => {
    if (!open) return setPos(null);
    const r = ref.current?.getBoundingClientRect();
    if (r) setPos({ top: r.bottom + 4, left: r.left });
  }, [open, ref]);

  const remove = async (force: boolean) => {
    setError(null);
    const wt = session.worktree;
    if (wt && !worktreeShared(session)) {
      try {
        await removeSessionWorktree(wt.root, wt.branch, force);
      } catch (e) {
        if (String(e) === "DIRTY") return setConfirm("dirty");
        return setError(String(e));
      }
    }
    await useChat.getState().cancel(session.id);
    useChat.getState().removeSession(session.id);
    deleteSession(session.id);
    setConfirm(null);
  };

  const itemClass =
    "flex h-8 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-hover disabled:cursor-not-allowed disabled:opacity-50";

  return (
    <span ref={ref} className="mr-1 inline-flex">
      <IconButton
        label={`${t("sidebar.session.actions")}: ${session.title}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "size-6",
          open ? "bg-hover text-fg" : "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100",
        )}
      >
        <Ellipsis className="size-3.5" />
      </IconButton>
      {open && pos && (
        <div
          role="menu"
          aria-label={t("sidebar.session.actions")}
          style={{ top: pos.top, left: pos.left }}
          className="fixed z-30 w-max min-w-40 rounded-lg border border-border bg-elevated p-1 text-[13px] shadow-lg"
        >
          <button
            type="button"
            role="menuitem"
            className={cn(itemClass, "text-fg")}
            onClick={() => {
              setOpen(false);
              onRename();
            }}
          >
            <Pencil aria-hidden className="size-3.5 text-muted" />
            {t("sidebar.session.rename")}
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={busy}
            title={busy ? t("sidebar.session.busy") : undefined}
            className={cn(itemClass, "text-fg")}
            onClick={() => {
              setOpen(false);
              forkSession(session, t("sidebar.session.forkSuffix"));
            }}
          >
            <GitFork aria-hidden className="size-3.5 text-muted" />
            {t("sidebar.session.fork")}
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={busy}
            title={busy ? t("sidebar.session.busy") : undefined}
            className={cn(itemClass, "text-fg")}
            onClick={() => {
              setOpen(false);
              setSessionArchived(session.id, !session.archived);
            }}
          >
            {session.archived ? (
              <ArchiveRestore aria-hidden className="size-3.5 text-muted" />
            ) : (
              <Archive aria-hidden className="size-3.5 text-muted" />
            )}
            {t(session.archived ? "sidebar.session.unarchive" : "sidebar.session.archive")}
          </button>
          <div className="my-1 border-t border-border" />
          <button
            type="button"
            role="menuitem"
            className={cn(itemClass, "text-danger")}
            onClick={() => {
              setOpen(false);
              setError(null);
              setConfirm("delete");
            }}
          >
            <Trash2 aria-hidden className="size-3.5" />
            {t("sidebar.session.delete")}
          </button>
        </div>
      )}
      {confirm && (
        <ConfirmDialog
          title={t(confirm === "dirty" ? "sidebar.session.dirtyTitle" : "sidebar.session.deleteTitle", {
            title: session.title,
          })}
          body={
            confirm === "dirty"
              ? t("sidebar.session.dirtyBody", { path: session.worktree?.root })
              : session.worktree && !worktreeShared(session)
                ? t("sidebar.session.deleteBodyWorktree", { branch: session.worktree.branch })
                : t("sidebar.session.deleteBody")
          }
          action={t(confirm === "dirty" ? "sidebar.session.deleteAnyway" : "sidebar.session.delete")}
          error={error}
          onConfirm={() => void remove(confirm === "dirty")}
          onCancel={() => setConfirm(null)}
        />
      )}
    </span>
  );
}

function ConfirmDialog({
  title,
  body,
  action,
  error,
  onConfirm,
  onCancel,
}: {
  title: string;
  body: string;
  action: string;
  error: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  return (
    <dialog
      ref={dialog}
      aria-labelledby="session-confirm-title"
      aria-describedby="session-confirm-body"
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
      className="m-auto w-[26rem] max-w-[calc(100vw-2rem)] rounded-xl border border-border bg-elevated p-0 text-fg shadow-xl backdrop:bg-black/40"
    >
      <div className="space-y-2 px-5 pt-5 pb-3">
        <h2 id="session-confirm-title" className="text-[14px] font-medium">
          {title}
        </h2>
        <p id="session-confirm-body" className="text-[13px] leading-relaxed text-muted" data-selectable>
          {body}
        </p>
        {error && (
          <p role="alert" className="text-xs text-danger" data-selectable>
            {error}
          </p>
        )}
      </div>
      <div className="flex justify-end gap-2 px-5 py-3">
        <Button onClick={onCancel} autoFocus>
          {t("sidebar.session.cancel")}
        </Button>
        <Button variant="primary" className="bg-danger" onClick={onConfirm}>
          {action}
        </Button>
      </div>
    </dialog>
  );
}
