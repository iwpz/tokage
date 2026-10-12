import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, Gauge, PanelLeft, PiggyBank } from "lucide-react";
import { cn } from "../lib/cn";
import { useProjects } from "../stores/projects";
import { useUi } from "../stores/ui";
import { useLayout } from "../lib/layout";
import { ProviderStatusMenu } from "./ProviderStatusMenu";
import { IconButton } from "./ui";
import { getUsageDashboard, type UsageDashboard } from "../lib/ipc";

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

  // Topbar Summary data (P8.m3d)
  const [summary, setSummary] = useState<UsageDashboard | null>(null);

  useEffect(() => {
    let active = true;
    getUsageDashboard("7d")
      .then((d) => {
        if (active) setSummary(d);
      })
      .catch(() => {});

    const interval = setInterval(() => {
      getUsageDashboard("7d")
        .then((d) => {
          if (active) setSummary(d);
        })
        .catch(() => {});
    }, 15000);

    return () => {
      active = false;
      clearInterval(interval);
    };
  }, []);

  const savedAmount = `$${(summary?.savings?.totalUsd ?? 0).toFixed(2)}`;
  const firstPassRate = Math.round((summary?.quality?.firstPassRate ?? 1.0) * 100);

  return (
    <header data-tauri-drag-region className="flex h-12 shrink-0 items-center border-b border-border bg-sidebar">
      {/* Left zone tracks the sidebar width so the title starts above the workspace. */}
      <div
        data-tauri-drag-region
        style={{ width: Math.max(docked, 124) }}
        className={cn(
          "pane-width flex h-full shrink-0 items-center pl-[84px]",
          docked > 0 && "border-r border-border",
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

      <div data-tauri-drag-region className="flex min-w-0 flex-1 items-center justify-between gap-3 px-4">
        <div data-tauri-drag-region className="flex min-w-0 items-baseline gap-2">
          {title && <span className="min-w-0 shrink truncate text-[13px] font-semibold max-w-[200px]">{title}</span>}
          {subtitle && (
            <span className={cn("hidden truncate text-xs text-subtle sm:inline", !session && "font-mono")}>
              {subtitle}
            </span>
          )}
        </div>

        {/* Topbar Summary (P8.m3d) */}
        <div data-tauri-drag-region className="hidden items-center gap-2 md:flex">
          {/* Week Savings Pill */}
          {summary && (
            <button
              type="button"
              onClick={openDashboard}
              title={t("topbar.savedThisWeekTitle", { amount: savedAmount })}
              className="flex items-center gap-1.5 rounded-full border border-accent/30 bg-accent/10 px-2.5 py-0.5 text-[11px] font-medium text-accent hover:bg-accent/20"
            >
              <PiggyBank className="size-3" />
              <span>{t("topbar.savedThisWeek", { amount: savedAmount })}</span>
            </button>
          )}

          {/* First Pass Rate Pill */}
          {summary && (
            <button
              type="button"
              onClick={openDashboard}
              title={t("topbar.firstPassTitle", { rate: firstPassRate })}
              className="flex items-center gap-1.5 rounded-full border border-border bg-elevated/80 px-2 py-0.5 text-[11px] font-medium text-subtle hover:text-fg"
            >
              <CheckCircle2 className="size-3 text-emerald-500" />
              <span>{t("topbar.firstPass", { rate: firstPassRate })}</span>
            </button>
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


