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
      ? t("dashboard.title", "用量与质量看板")
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
          {/* Provider Quota Pills */}
          {summary?.providers.slice(0, 4).map((p) => {
            const pct = p.usagePct;
            const pctStr = pct !== undefined ? `${Math.round(pct * 100)}%` : p.spentUsd ? `$${p.spentUsd.toFixed(2)}` : "";
            const dotColor =
              p.status === "MELTED" || (pct !== undefined && pct >= 0.95)
                ? "bg-rose-500"
                : pct !== undefined && pct >= 0.8
                ? "bg-amber-500"
                : "bg-emerald-500";

            return (
              <button
                key={p.provider}
                type="button"
                onClick={openDashboard}
                title={`${p.provider}: ${p.status} ${pctStr ? `(${pctStr})` : ""}`}
                className="flex items-center gap-1.5 rounded-full border border-border/80 bg-elevated/80 px-2 py-0.5 text-[11px] font-medium text-muted hover:border-accent hover:text-fg"
              >
                <span className={cn("size-1.5 rounded-full", dotColor)} />
                <span className="truncate max-w-[70px]">{shortProviderName(p.provider)}</span>
                {pctStr && <span className="font-mono text-[10px] text-fg">{pctStr}</span>}
              </button>
            );
          })}

          {/* Week Savings Pill */}
          {summary && (
            <button
              type="button"
              onClick={openDashboard}
              title={`本周节省: $${summary.savings.totalUsd.toFixed(2)} (点击查看用量看板)`}
              className="flex items-center gap-1.5 rounded-full border border-accent/30 bg-accent/10 px-2.5 py-0.5 text-[11px] font-medium text-accent hover:bg-accent/20"
            >
              <PiggyBank className="size-3" />
              <span>本周已省 ${summary.savings.totalUsd.toFixed(2)}</span>
            </button>
          )}

          {/* First Pass Rate Pill */}
          {summary && (
            <button
              type="button"
              onClick={openDashboard}
              title={`全部任务首次通过率: ${Math.round(summary.quality.firstPassRate * 100)}%`}
              className="flex items-center gap-1.5 rounded-full border border-border bg-elevated/80 px-2 py-0.5 text-[11px] font-medium text-subtle hover:text-fg"
            >
              <CheckCircle2 className="size-3 text-emerald-500" />
              <span>首通率 {Math.round(summary.quality.firstPassRate * 100)}%</span>
            </button>
          )}
        </div>

        <div className="flex items-center gap-2">
          <IconButton label={t("dashboard.title", "用量看板")} onClick={openDashboard} className="size-7">
            <Gauge className="size-3.5" />
          </IconButton>
          <ProviderStatusMenu />
        </div>
      </div>
    </header>
  );
}

function shortProviderName(id: string): string {
  switch (id) {
    case "CLAUDE_CODE":
      return "Claude";
    case "CODEX_CLI":
      return "Codex";
    case "CURSOR_CLI":
      return "Cursor";
    case "COPILOT_CLI":
      return "Copilot";
    case "ANTIGRAVITY_CLI":
      return "Agy";
    case "AIDER":
      return "Aider";
    case "OPENCODE":
      return "OpenCode";
    default:
      return id.slice(0, 6);
  }
}


