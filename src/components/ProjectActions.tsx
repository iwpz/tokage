import { useEffect, useLayoutEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Code, Download, Ellipsis, Eraser, FolderOpen, SquareTerminal, Trash2 } from "lucide-react";
import { cn } from "../lib/cn";
import {
  clearProjectRecords,
  exportProjectData,
  listOpenTargets,
  openPathIn,
  removeTokageDir,
  type OpenTarget,
} from "../lib/ipc";
import { usePopover } from "../lib/usePopover";
import type { Project } from "../stores/projects";
import { Button, IconButton, Modal } from "./ui";

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
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [showRemoveModal, setShowRemoveModal] = useState(false);
  const [cleanTokageDir, setCleanTokageDir] = useState(true);

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
            onClick={async () => {
              setOpen(false);
              try {
                const data = await exportProjectData(project.id);
                const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
                const url = URL.createObjectURL(blob);
                const a = document.createElement("a");
                a.href = url;
                a.download = `tokage-project-${project.name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}.json`;
                document.body.appendChild(a);
                a.click();
                document.body.removeChild(a);
                URL.revokeObjectURL(url);
              } catch (e) {
                setError(String(e));
              }
            }}
            className={cn(itemClass, "text-fg")}
          >
            <Download aria-hidden className="size-3.5 text-muted" />
            {t("sidebar.exportProject")}
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              setShowClearConfirm(true);
            }}
            className={cn(itemClass, "text-amber-600 dark:text-amber-400")}
          >
            <Eraser aria-hidden className="size-3.5" />
            {t("sidebar.clearProjectRecords")}
          </button>
          <div className="my-1 border-t border-border" />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              setShowRemoveModal(true);
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

      {/* Clear Records Confirmation Modal */}
      <Modal
        open={showClearConfirm}
        onClose={() => setShowClearConfirm(false)}
        title={t("sidebar.clearProjectConfirmTitle", { name: project.name })}
        className="max-w-md"
      >
        <div className="space-y-4">
          <p className="text-xs text-muted leading-relaxed">
            {t("sidebar.clearProjectConfirmBody")}
          </p>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" size="sm" onClick={() => setShowClearConfirm(false)}>
              {t("sidebar.session.cancel")}
            </Button>
            <Button
              variant="primary"
              size="sm"
              className="bg-danger text-danger-fg hover:opacity-90"
              onClick={async () => {
                setShowClearConfirm(false);
                try {
                  await clearProjectRecords(project.id);
                  // Refresh window or store if needed
                  window.location.reload();
                } catch (e) {
                  setError(String(e));
                }
              }}
            >
              {t("sidebar.clearProjectRecords")}
            </Button>
          </div>
        </div>
      </Modal>

      {/* Remove Project Modal (with .tokage/ cleanup option) */}
      <Modal
        open={showRemoveModal}
        onClose={() => setShowRemoveModal(false)}
        title={t("sidebar.removeProjectConfirmTitle", { name: project.name })}
        className="max-w-md"
      >
        <div className="space-y-4">
          <p className="text-xs text-muted leading-relaxed">
            {t("sidebar.removeProjectConfirmBody")}
          </p>
          <label className="flex items-center gap-2 text-xs text-fg cursor-pointer select-none">
            <input
              type="checkbox"
              checked={cleanTokageDir}
              onChange={(e) => setCleanTokageDir(e.target.checked)}
              className="rounded border-border text-accent focus:ring-accent"
            />
            <span>{t("sidebar.cleanTokageDirCheckbox")}</span>
          </label>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" size="sm" onClick={() => setShowRemoveModal(false)}>
              {t("sidebar.session.cancel")}
            </Button>
            <Button
              variant="primary"
              size="sm"
              className="bg-danger text-danger-fg hover:opacity-90"
              onClick={async () => {
                setShowRemoveModal(false);
                if (cleanTokageDir) {
                  try {
                    await removeTokageDir(project.path);
                  } catch (err) {
                    console.error("Failed to remove .tokage directory:", err);
                  }
                }
                onRemove();
              }}
            >
              {t("sidebar.removeProject")}
            </Button>
          </div>
        </div>
      </Modal>
    </span>
  );
}
