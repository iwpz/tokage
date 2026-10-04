import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Check, FolderPlus } from "lucide-react";
import { cn } from "../lib/cn";
import { pickProjectDirectory } from "../lib/ipc";
import { useProjects } from "../stores/projects";
import { usePopover } from "../lib/usePopover";

/**
 * Dropdown for switching the active project or adding a new one.
 * `trigger` renders the button content; `placement` picks the side the menu opens on.
 */
export function ProjectMenu({
  trigger,
  triggerClassName,
  placement = "bottom",
}: {
  trigger: ReactNode;
  triggerClassName?: string;
  placement?: "bottom" | "top";
}) {
  const { t } = useTranslation();
  const { projects, activeProjectId, selectProject, addProject } = useProjects();
  const { open, setOpen, ref: rootRef } = usePopover();

  const itemClass = "flex h-8 w-full items-center gap-2 rounded-md px-2 text-left";

  return (
    <span ref={rootRef} className="relative inline-block">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className={triggerClassName}
      >
        {trigger}
      </button>
      {open && (
        <div
          role="menu"
          aria-label={t("workspace.switchProject")}
          className={cn(
            "absolute left-0 z-20 w-max min-w-40 max-w-60 rounded-lg border border-border bg-elevated p-1 text-left text-[13px] font-normal shadow-lg",
            placement === "bottom" ? "top-full mt-2" : "bottom-full mb-2",
          )}
        >
          {projects.map((p) => (
            <button
              key={p.id}
              type="button"
              role="menuitemradio"
              aria-checked={p.id === activeProjectId}
              title={p.path}
              onClick={() => {
                selectProject(p.id);
                setOpen(false);
              }}
              className={cn(itemClass, "text-fg hover:bg-hover")}
            >
              <span className="min-w-0 flex-1 truncate">{p.name}</span>
              <Check aria-hidden className={cn("size-3.5 text-muted", p.id !== activeProjectId && "invisible")} />
            </button>
          ))}
          <div className="my-1 border-t border-border" />
          <button
            type="button"
            role="menuitem"
            onClick={async () => {
              setOpen(false);
              const path = await pickProjectDirectory();
              if (path) addProject(path);
            }}
            className={cn(itemClass, "text-muted hover:bg-hover hover:text-fg")}
          >
            <FolderPlus aria-hidden className="size-3.5" />
            {t("sidebar.addProject")}
          </button>
        </div>
      )}
    </span>
  );
}
