import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, Check, GitBranch, GitFork, Plus } from "lucide-react";
import { cn } from "../lib/cn";
import { Logo } from "./Logo";
import { useSettings } from "../stores/settings";
import {
  gitCheckoutBranch,
  gitCreateBranch,
  gitFileDiff,
  gitListBranches,
  gitStatusFiles,
  type FileChangeKind,
  type GitBranchInfo,
  type GitInfo,
  type GitStatusFile,
} from "../lib/ipc";
import { usePopover } from "../lib/usePopover";
import { useUi } from "../stores/ui";

function toPreviewChange(change: string): FileChangeKind | null {
  if (change === "added" || change === "untracked") return "added";
  if (change === "deleted") return "deleted";
  if (change === "modified" || change === "renamed" || change === "conflicted") return "modified";
  return "modified";
}

/** Branch switcher: list local branches, switch, or create. Dirty trees are blocked. */
function BranchPicker({
  cwd,
  git,
  onChanged,
}: {
  cwd: string;
  git: GitInfo;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const { open, setOpen, ref } = usePopover<HTMLDivElement>();
  const [branches, setBranches] = useState<GitBranchInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    gitListBranches(cwd)
      .then((list) => !cancelled && setBranches(list))
      .catch((e) => !cancelled && setError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [open, cwd]);

  const switchTo = async (name: string) => {
    if (name === git.branch && !git.detached) {
      setOpen(false);
      return;
    }
    setError(null);
    try {
      await gitCheckoutBranch(cwd, name);
      setOpen(false);
      onChanged();
    } catch (e) {
      const msg = String(e);
      setError(msg === "DIRTY" || msg.includes("DIRTY") ? t("workspace.git.dirtyBlock") : msg);
    }
  };

  const create = async () => {
    const name = newName.trim();
    if (!name || creating) return;
    setCreating(true);
    setError(null);
    try {
      await gitCreateBranch(cwd, name);
      setNewName("");
      setOpen(false);
      onChanged();
    } catch (e) {
      const msg = String(e);
      setError(msg === "DIRTY" || msg.includes("DIRTY") ? t("workspace.git.dirtyBlock") : msg);
    } finally {
      setCreating(false);
    }
  };

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        aria-expanded={open}
        aria-haspopup="listbox"
        onClick={() => setOpen((v) => !v)}
        className="inline-flex h-6 min-w-0 max-w-full items-center gap-1.5 rounded-md px-1.5 hover:bg-hover hover:text-fg"
        title={[
          git.detached ? t("workspace.git.detached") : t("workspace.git.branch"),
          git.upstream ? t("workspace.git.tracking", { upstream: git.upstream }) : t("workspace.git.noUpstream"),
          git.linkedWorktree ? t("workspace.git.inWorktree") : null,
        ]
          .filter(Boolean)
          .join("\n")}
      >
        {git.linkedWorktree ? (
          <GitFork aria-hidden className="size-3.5 shrink-0" />
        ) : (
          <GitBranch aria-hidden className="size-3.5 shrink-0" />
        )}
        <span className="max-w-48 truncate font-mono">{git.branch ?? t("workspace.git.noBranch")}</span>
        {git.detached && <span className="text-warning">{t("workspace.git.detachedShort")}</span>}
        {!git.hasCommits && <span className="text-subtle">{t("workspace.git.noCommits")}</span>}
        {(git.ahead > 0 || git.behind > 0) && (
          <span
            className="inline-flex items-center gap-0.5 tabular-nums"
            aria-label={t("workspace.git.aheadBehind", { ahead: git.ahead, behind: git.behind })}
          >
            {git.ahead > 0 && (
              <>
                <ArrowUp aria-hidden className="size-3" />
                {git.ahead}
              </>
            )}
            {git.behind > 0 && (
              <>
                <ArrowDown aria-hidden className="size-3" />
                {git.behind}
              </>
            )}
          </span>
        )}
      </button>
      {open && (
        <div
          role="listbox"
          aria-label={t("workspace.git.branches")}
          className="absolute bottom-full left-0 z-30 mb-1 w-64 overflow-hidden rounded-lg border border-border bg-elevated shadow-lg"
        >
          <div className="max-h-56 overflow-y-auto py-1">
            {loading && <p className="px-3 py-2 text-xs text-subtle">{t("workspace.git.loading")}</p>}
            {!loading &&
              branches.map((b) => (
                <button
                  key={b.name}
                  type="button"
                  role="option"
                  aria-selected={b.current}
                  disabled={!git.hasCommits}
                  onClick={() => void switchTo(b.name)}
                  className={cn(
                    "flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-hover",
                    b.current && "text-accent",
                  )}
                >
                  <span className="w-3.5 shrink-0">{b.current && <Check className="size-3.5" />}</span>
                  <span className="min-w-0 truncate font-mono">{b.name}</span>
                </button>
              ))}
            {!loading && branches.length === 0 && (
              <p className="px-3 py-2 text-xs text-subtle">{t("workspace.git.noBranches")}</p>
            )}
          </div>
          <div className="border-t border-border p-2">
            <div className="flex items-center gap-1">
              <input
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void create();
                  }
                }}
                placeholder={t("workspace.git.newBranchPlaceholder")}
                disabled={!git.hasCommits || creating}
                className="min-w-0 flex-1 rounded-md border border-border bg-bg px-2 py-1 font-mono text-xs text-fg placeholder:text-subtle focus:border-ring focus:outline-none"
              />
              <button
                type="button"
                aria-label={t("workspace.git.createBranch")}
                disabled={!newName.trim() || !git.hasCommits || creating}
                onClick={() => void create()}
                className="inline-flex size-7 shrink-0 items-center justify-center rounded-md bg-hover text-fg hover:bg-border disabled:opacity-40"
              >
                <Plus className="size-3.5" />
              </button>
            </div>
          </div>
          {error && (
            <p role="alert" className="border-t border-border px-3 py-2 text-xs text-warning">
              {error}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/** Changes popover: `path +N −N` rows; click opens right-hand preview with diff. */
function ChangesPicker({ cwd, git, onChanged }: { cwd: string; git: GitInfo; onChanged: () => void }) {
  const { t } = useTranslation();
  const { open, setOpen, ref } = usePopover<HTMLDivElement>();
  const openPreview = useUi((s) => s.openPreview);
  const [files, setFiles] = useState<GitStatusFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState<string | null>(null);

  const detail = [
    git.staged ? t("workspace.git.staged", { count: git.staged }) : null,
    git.unstaged ? t("workspace.git.unstaged", { count: git.unstaged }) : null,
    git.untracked ? t("workspace.git.untracked", { count: git.untracked }) : null,
    git.conflicted ? t("workspace.git.conflicted", { count: git.conflicted }) : null,
  ]
    .filter(Boolean)
    .join(" · ");

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    gitStatusFiles(cwd)
      .then((list) => !cancelled && setFiles(list))
      .catch((e) => !cancelled && setError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [open, cwd]);

  const openFile = async (file: GitStatusFile) => {
    setOpening(file.path);
    setError(null);
    try {
      const d = await gitFileDiff(cwd, file.path);
      openPreview({
        root: cwd,
        path: d.path,
        diff: d.diff,
        change: toPreviewChange(d.change),
      });
      setOpen(false);
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setOpening(null);
    }
  };

  if (git.changes <= 0) {
    return git.hasCommits ? <span className="px-1.5 text-subtle">{t("workspace.git.clean")}</span> : null;
  }

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        aria-expanded={open}
        aria-haspopup="listbox"
        onClick={() => setOpen((v) => !v)}
        className="inline-flex h-6 items-center gap-1.5 rounded-md px-1.5 hover:bg-hover hover:text-fg"
        title={detail}
      >
        <span className="text-warning">{t("workspace.git.changes", { count: git.changes })}</span>
        {(git.insertions > 0 || git.deletions > 0) && (
          <span className="font-mono tabular-nums">
            <span className="text-success">+{git.insertions}</span>{" "}
            <span className="text-danger">−{git.deletions}</span>
          </span>
        )}
      </button>
      {open && (
        <div
          role="listbox"
          aria-label={t("workspace.git.changedFiles")}
          className="absolute bottom-full left-0 z-30 mb-1 w-80 overflow-hidden rounded-lg border border-border bg-elevated shadow-lg"
        >
          <div className="max-h-64 overflow-y-auto py-1">
            {loading && <p className="px-3 py-2 text-xs text-subtle">{t("workspace.git.loading")}</p>}
            {!loading &&
              files.map((f) => (
                <button
                  key={f.path}
                  type="button"
                  role="option"
                  disabled={opening === f.path}
                  onClick={() => void openFile(f)}
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-hover disabled:opacity-50"
                  title={f.path}
                >
                  <span className="min-w-0 flex-1 truncate font-mono">{f.path}</span>
                  <span className="shrink-0 font-mono tabular-nums">
                    <span className="text-success">+{f.insertions}</span>{" "}
                    <span className="text-danger">−{f.deletions}</span>
                  </span>
                </button>
              ))}
            {!loading && files.length === 0 && (
              <p className="px-3 py-2 text-xs text-subtle">{t("workspace.git.clean")}</p>
            )}
          </div>
          {error && (
            <p role="alert" className="border-t border-border px-3 py-2 text-xs text-warning">
              {error}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/** Interactive branch + changes controls for the composer footer. */
export function GitStatusBar({
  cwd,
  git,
  onChanged,
}: {
  cwd: string;
  git: GitInfo;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const tokageCoAuthor = useSettings((s) => s.tokageCoAuthor);
  const showCoAuthorBadge = tokageCoAuthor || !!git.headCoAuthoredByTokage;

  return (
    <>
      <BranchPicker cwd={cwd} git={git} onChanged={onChanged} />
      <ChangesPicker cwd={cwd} git={git} onChanged={onChanged} />
      {showCoAuthorBadge && (
        <span
          className="inline-flex h-6 items-center gap-1 rounded-md px-1.5 text-subtle"
          title={
            git.headAuthorName && git.headCoAuthoredByTokage
              ? t("workspace.git.coAuthorWithAuthor", { author: git.headAuthorName })
              : t("workspace.git.coAuthorActive")
          }
        >
          <Logo className="size-3.5 shrink-0 rounded-xs" />
          <span className="text-[11px]">{t("workspace.git.coAuthorBadge")}</span>
        </span>
      )}
    </>
  );
}
