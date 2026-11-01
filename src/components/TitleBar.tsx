import { useTranslation } from "react-i18next";
import { Gauge, PanelLeft } from "lucide-react";
import { cn } from "../lib/cn";
import { useProjects } from "../stores/projects";
import { useUi } from "../stores/ui";
import { useLayout } from "../lib/layout";
import { ProviderStatusMenu } from "./ProviderStatusMenu";
import { IconButton } from "./ui";

/**
 * Full-width window title bar. It sits above both the sidebar and the workspace,
 * hosts the macOS traffic lights (titleBarStyle: Overlay, trafficLightPosition in
 * tauri.conf.json) and the sidebar toggle right of them.
 */
export function TitleBar() {
  const { t } = useTranslation();
  const { view, toggleSidebar, openDashboard } = useUi();
  const { docked } = useLayout();
  const collapsed = useUi((s) => (s.compact ? !s.drawerOpen : s.sidebarCollapsed));
  const project = useProjects((s) => s.projects.find((p) => p.id === s.activeProjectId) ?? null);
  const activeSessionId = useProjects((s) => s.activeSessionId);
  const session = project?.sessions.find((s) => s.id === activeSessionId) ?? null;

  // Title bar title
  const title =
    view === "settings"
      ? t("settings.title")
      : view === "dashboard"
      ? t("dashboard.title")
      : (session?.title ?? project?.name);
  const subtitle =
    view === "settings" || view === "dashboard"
      ? undefined
      : session
      ? project?.name
      : project?.path;

  return (
    <header data-tauri-drag-region className="relative z-40 flex h-12 shrink-0 items-center">
      {/* Left zone tracks the sidebar width so the title starts above the workspace. */}
      <div
        data-tauri-drag-region
        style={{ width: Math.max(docked, 124) }}
        className={cn(
          "pane-width flex h-full shrink-0 items-center pl-[84px]",
          docked > 0
            ? "border-r border-line surface-glass-sidebar"
            : "surface-glass-titlebar",
        )}
      >
        <IconButton
          label={collapsed ? t("sidebar.expand") : t("sidebar.collapse")}
          aria-expanded={!collapsed}
          onClick={toggleSidebar}
        >
          <PanelLeft className="size-4" />
        </IconButton>
      </div>

      <div
        data-tauri-drag-region
        className="flex h-full min-w-0 flex-1 items-center justify-between gap-3 px-4 surface-glass-titlebar"
      >
        <div data-tauri-drag-region className="flex min-w-0 items-baseline gap-2">
          {title && <span className="min-w-0 max-w-[260px] shrink truncate text-[13.5px] font-semibold tracking-[-0.01em]">{title}</span>}
          {subtitle && (
            <span data-selectable className={cn("hidden truncate text-xs text-subtle sm:inline", !session && "font-mono")}>
              {subtitle}
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          <IconButton label={t("dashboard.title")} onClick={openDashboard} className="size-7">
            <Gauge className="size-3.5" />
          </IconButton>
          <ProviderStatusMenu />
        </div>
      </div>
    </header>
  );
}
