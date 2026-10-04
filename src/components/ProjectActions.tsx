import { useEffect, useLayoutEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Code, Ellipsis, FolderOpen, SquareTerminal, Trash2 } from "lucide-react";
import { cn } from "../lib/cn";
import { listOpenTargets, openPathIn, type OpenTarget } from "../lib/ipc";
import { usePopover } from "../lib/usePopover";
import type { Project } from "../stores/projects";
import { IconButton } from "./ui";

const OPEN_TARGET_ICONS: Record<OpenTarget, typeof FolderOpen> = {
  FILE_MANAGER: FolderOpen,
  VS_CODE: Code,
  TERMINAL: SquareTerminal,
};

const isMac = navigator.userAgent.includes("Mac");
const isWindows = navigator.userAgent.includes("Windows");

/**
 * "⋯" button on a sidebar project row: open the folder in other apps, remove the project.
 * The menu is position: fixed so the scrolling sidebar doesn't clip it.
 */
export function ProjectActions({ project, onRemove }: { project: Project; onRemove: () => void }) {
  const { t } = useTranslation();
  const { open, setOpen, ref } = usePopover();
  const [targets, setTargets] = useState<OpenTarget[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);

  // Re-detect each time the menu opens so a freshly installed VS Code shows up.
  useEffect(() => {
    if (!open) return;
    setError(null);
    listOpenTargets()
      .then(setTargets)
      .catch(() => setTargets(["FILE_MANAGER", "TERMINAL"]));
  }, [open]);

  useLayoutEffect(() => {
    if (!open) return setPos(null);
    const r = ref.current?.getBoundingClientRect();
    if (r) setPos({ top: r.bottom + 4, left: r.left });
  }, [open, ref]);

  const label = (target: OpenTarget) =>
    target === "FILE_MANAGER"
      ? t(isMac ? "titlebar.openIn.finder" : isWindows ? "titlebar.openIn.explorer" : "titlebar.openIn.fileManager")
      : t(target === "VS_CODE" ? "titlebar.openIn.vscode" : "titlebar.openIn.terminal");

  const itemClass = "flex h-8 w-full items-center gap-2 rounded-md px-2 text-left hover:bg-hover";

  return (
    <span ref={ref} className="mr-1 inline-flex">
      <IconButton
        label={`${t("sidebar.projectActions")}: ${project.name}`}
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
          aria-label={t("sidebar.projectActions")}
          style={{ top: pos.top, left: pos.left }}
          className="fixed z-30 w-max min-w-44 rounded-lg border border-border bg-elevated p-1 text-[13px] shadow-lg"
        >
          {targets.map((target) => {
            const Icon = OPEN_TARGET_ICONS[target];
            return (
              <button
                key={target}
                type="button"
                role="menuitem"
                onClick={() => {
                  openPathIn(project.path, target)
                    .then(() => setOpen(false))
                    .catch((e) => setError(String(e)));
                }}
                className={cn(itemClass, "text-fg")}
              >
                <Icon aria-hidden className="size-3.5 text-muted" />
                {label(target)}
              </button>
            );
          })}
          <div className="my-1 border-t border-border" />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              onRemove();
            }}
            className={cn(itemClass, "text-danger")}
          >
            <Trash2 aria-hidden className="size-3.5" />
            {t("sidebar.removeProject")}
          </button>
          {error && (
            <p role="alert" className="max-w-56 px-2 py-1 text-xs text-danger">
              {error}
            </p>
          )}
        </div>
      )}
    </span>
  );
}
