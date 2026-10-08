import { useTranslation } from "react-i18next";
import {
  ChevronRight,
  Folder,
  FolderOpen,
  FolderPlus,
  GitFork,
  MessageSquare,
  Settings,
  SquarePen,
  ArrowLeft,
  SlidersHorizontal,
  TerminalSquare,
  Network,
  Layers,
  Gauge,
  Play,
  Package,
  Info,
  Loader2,
  Plus,
  ShieldCheck,
  Globe,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { cn } from "../lib/cn";
import { pickProjectDirectory } from "../lib/ipc";
import { useProjects, type MainSession, type Project } from "../stores/projects";
import { isStreaming, useChat } from "../stores/chat";
import { SIDEBAR_WIDTH, useUi, type SettingsSection } from "../stores/ui";
import { sidebarMax, useLayout } from "../lib/layout";
import { IconButton } from "./ui";
import { Logo } from "./Logo";
import { ProjectActions } from "./ProjectActions";
import { SessionActions } from "./SessionActions";
import { Splitter } from "./Splitter";

export function Sidebar() {
  const { t } = useTranslation();
  const { win, compact, sidebarWidth } = useLayout();
  const view = useUi((s) => s.view);
  const drawerOpen = useUi((s) => s.drawerOpen);
  const closeDrawer = useUi((s) => s.closeDrawer);
  const setWidth = useUi((s) => s.setSidebarWidth);
  const collapsed = useUi((s) => (s.compact ? !s.drawerOpen : s.sidebarCollapsed));
  const activeSessionId = useProjects((s) => s.activeSessionId);
  const activeProjectId = useProjects((s) => s.activeProjectId);
  const section = useUi((s) => s.settingsSection);

  // The drawer gets out of the way once something in it has been picked.
  useEffect(() => closeDrawer(), [activeSessionId, activeProjectId, view, section, closeDrawer]);

  useEffect(() => {
    if (!drawerOpen) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && closeDrawer();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [drawerOpen, closeDrawer]);

  const content = (
    <>
      <SidebarHeader />
      {view === "settings" ? <SettingsNav /> : <ProjectsNav />}
    </>
  );

  if (compact) {
    return (
      <>
        {drawerOpen && (
          <div aria-hidden className="absolute inset-0 z-30 bg-black/25" onClick={closeDrawer} />
        )}
        <aside
          aria-hidden={collapsed}
          inert={collapsed}
          style={{ width: sidebarWidth }}
          className={cn(
            "absolute inset-y-0 left-0 z-40 flex max-w-[85vw] flex-col overflow-hidden border-r border-border bg-sidebar shadow-xl transition-transform duration-200",
            collapsed && "-translate-x-full shadow-none",
          )}
        >
          {content}
        </aside>
      </>
    );
  }

  return (
    <aside
      aria-hidden={collapsed}
      inert={collapsed}
      style={{ width: collapsed ? 0 : sidebarWidth }}
      className={cn(
        "pane-width relative flex h-full shrink-0 flex-col border-r border-border bg-sidebar",
        collapsed && "border-r-0",
      )}
    >
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{content}</div>
      {!collapsed && (
        <Splitter
          label={t("sidebar.resize")}
          side="left"
          value={sidebarWidth}
          min={SIDEBAR_WIDTH.min}
          max={sidebarMax(win)}
          onChange={setWidth}
          onReset={() => setWidth(SIDEBAR_WIDTH.default)}
        />
      )}
    </aside>
  );
}

function SidebarHeader() {
  const { t } = useTranslation();
  return (
    <div className="flex h-10 shrink-0 items-center gap-1.5 px-4 text-[13px] font-semibold text-fg">
      <Logo className="size-4" />
      {t("app.name")}
    </div>
  );
}

function NavItem({
  icon,
  label,
  active,
  onClick,
  trailing,
  indent,
}: {
  icon?: ReactNode;
  label: string;
  active?: boolean;
  onClick: () => void;
  trailing?: ReactNode;
  indent?: boolean;
}) {
  return (
    <div
      className={cn(
        "group flex h-8 items-center rounded-md text-[13px] transition-colors",
        active ? "bg-active text-fg" : "text-muted hover:bg-hover hover:text-fg",
      )}
    >
      <button
        type="button"
        onClick={onClick}
        aria-current={active ? "page" : undefined}
        className={cn("flex h-full min-w-0 flex-1 items-center gap-2 px-2 text-left", indent && "pl-8")}
      >
        {icon && <span className="shrink-0">{icon}</span>}
        <span className="truncate">{label}</span>
      </button>
      {trailing}
    </div>
  );
}

function ProjectsNav() {
  const { t } = useTranslation();
  const { projects, activeProjectId, activeSessionId, addProject, removeProject, toggleProject, selectProject } =
    useProjects();
  const openSettings = useUi((s) => s.openSettings);
  const openDashboard = useUi((s) => s.openDashboard);

  const onAdd = async () => {
    const path = await pickProjectDirectory();
    if (path) addProject(path);
  };

  return (
    <>
      <div className="px-2 pb-2">
        <NavItem
          icon={<SquarePen className="size-4" />}
          label={t("sidebar.newRequest")}
          active={false}
          onClick={() => activeProjectId && selectProject(activeProjectId)}
        />
      </div>

      <nav aria-label={t("sidebar.projects")} className="min-h-0 flex-1 overflow-y-auto px-2">
        <div className="flex h-7 items-center justify-between px-2">
          <span className="text-xs font-medium text-subtle">{t("sidebar.projects")}</span>
          <IconButton label={t("sidebar.addProject")} onClick={onAdd} className="size-6">
            <FolderPlus className="size-3.5" />
          </IconButton>
        </div>

        {projects.length === 0 ? (
          <p className="px-2 py-1 text-xs text-subtle">{t("sidebar.noProjects")}</p>
        ) : (
          <ul className="space-y-0.5">
            {projects.map((p) => {
              const isActive = p.id === activeProjectId;
              return (
                <li key={p.id}>
                  <NavItem
                    icon={
                      <span className="flex items-center">
                        <ChevronRight
                          aria-hidden
                          className={cn("size-3 transition-transform", p.expanded && "rotate-90")}
                          onClick={(e) => {
                            e.stopPropagation();
                            toggleProject(p.id);
                          }}
                        />
                        {p.expanded ? (
                          <FolderOpen className="ml-1 size-4" />
                        ) : (
                          <Folder className="ml-1 size-4" />
                        )}
                      </span>
                    }
                    label={p.name}
                    active={isActive && !activeSessionId}
                    onClick={() => {
                      if (isActive) toggleProject(p.id);
                      selectProject(p.id);
                    }}
                    trailing={
                      <>
                        <IconButton
                          label={`${t("sidebar.newSession")}: ${p.name}`}
                          onClick={() => {
                            if (!p.expanded) toggleProject(p.id);
                            selectProject(p.id);
                          }}
                          className="size-6 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
                        >
                          <Plus className="size-3.5" />
                        </IconButton>
                        <ProjectActions project={p} onRemove={() => removeProject(p.id)} />
                      </>
                    }
                  />
                  {p.expanded && <SessionList project={p} />}
                </li>
              );
            })}
          </ul>
        )}
      </nav>

      <div className="border-t border-border p-2 space-y-0.5">
        <NavItem icon={<Gauge className="size-4" />} label={t("sidebar.dashboard", "用量看板")} onClick={() => openDashboard()} />
        <NavItem icon={<Settings className="size-4" />} label={t("sidebar.settings")} onClick={() => openSettings()} />
      </div>
    </>
  );
}

/** A project's sessions; archived ones sit behind a toggle at the bottom. */
function SessionList({ project }: { project: Project }) {
  const { t } = useTranslation();
  const [showArchived, setShowArchived] = useState(false);
  const live = project.sessions.filter((s) => !s.archived);
  const archived = project.sessions.filter((s) => s.archived);
  return (
    <ul className="mt-0.5 space-y-0.5">
      {live.length === 0 && <li className="py-1 pl-10 text-xs text-subtle">{t("sidebar.noSessions")}</li>}
      {live.map((s) => (
        <SessionRow key={s.id} projectId={project.id} session={s} />
      ))}
      {archived.length > 0 && (
        <li>
          <button
            type="button"
            aria-expanded={showArchived}
            onClick={() => setShowArchived((v) => !v)}
            className="flex h-7 w-full items-center gap-1 rounded-md pl-8 text-left text-xs text-subtle hover:bg-hover hover:text-fg"
          >
            <ChevronRight aria-hidden className={cn("size-3 transition-transform", showArchived && "rotate-90")} />
            {t("sidebar.session.archived", { count: archived.length })}
          </button>
        </li>
      )}
      {showArchived &&
        archived.map((s) => <SessionRow key={s.id} projectId={project.id} session={s} />)}
    </ul>
  );
}

function SessionRow({ projectId, session }: { projectId: string; session: MainSession }) {
  const { t } = useTranslation();
  const active = useProjects((s) => s.activeSessionId === session.id);
  const running = useChat((s) => isStreaming(s.conversations[session.id]));
  const { selectSession, renameSession } = useProjects();
  const [editing, setEditing] = useState(false);

  if (editing) {
    const commit = (value: string) => {
      const title = value.trim();
      if (title && title !== session.title) renameSession(session.id, title);
      setEditing(false);
    };
    return (
      <li className="flex h-8 items-center pl-8 pr-1">
        <input
          autoFocus
          defaultValue={session.title}
          aria-label={t("sidebar.session.rename")}
          onFocus={(e) => e.currentTarget.select()}
          onBlur={(e) => commit(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit(e.currentTarget.value);
            if (e.key === "Escape") {
              e.stopPropagation();
              setEditing(false);
            }
          }}
          className="h-7 w-full min-w-0 rounded-md border border-border bg-elevated px-2 text-[13px] text-fg outline-none focus:border-accent"
        />
      </li>
    );
  }

  return (
    <li className={cn(session.archived && "opacity-60")}>
      <NavItem
        indent
        icon={
          running ? (
            <Loader2 aria-label={t("sidebar.session.running")} className="size-3.5 animate-spin text-accent" />
          ) : session.forkedFrom ? (
            <GitFork aria-label={t("sidebar.session.forked")} className="size-3.5" />
          ) : (
            <MessageSquare className="size-3.5" />
          )
        }
        label={session.title}
        active={active}
        onClick={() => selectSession(projectId, session.id)}
        trailing={<SessionActions session={session} onRename={() => setEditing(true)} />}
      />
    </li>
  );
}

const SECTION_ICONS: Record<SettingsSection, ReactNode> = {
  general: <SlidersHorizontal className="size-4" />,
  cli: <TerminalSquare className="size-4" />,
  planning: <Network className="size-4" />,
  context: <Layers className="size-4" />,
  budget: <Gauge className="size-4" />,
  execution: <Play className="size-4" />,
  security: <ShieldCheck className="size-4" />,
  proxy: <Globe className="size-4" />,
  knowledge: <Package className="size-4" />,
  about: <Info className="size-4" />,
};

export const SETTINGS_SECTIONS = Object.keys(SECTION_ICONS) as SettingsSection[];

function SettingsNav() {
  const { t } = useTranslation();
  const { settingsSection, setSettingsSection, closeSettings } = useUi();

  return (
    <>
      <div className="px-2 pb-2">
        <NavItem icon={<ArrowLeft className="size-4" />} label={t("settings.back")} onClick={closeSettings} />
      </div>
      <nav aria-label={t("settings.title")} className="min-h-0 flex-1 overflow-y-auto px-2">
        <div className="flex h-7 items-center px-2 text-xs font-medium text-subtle">{t("settings.title")}</div>
        <ul className="space-y-0.5">
          {SETTINGS_SECTIONS.map((s) => (
            <li key={s}>
              <NavItem
                icon={SECTION_ICONS[s]}
                label={t(`settings.sections.${s}`)}
                active={s === settingsSection}
                onClick={() => setSettingsSection(s)}
              />
            </li>
          ))}
        </ul>
      </nav>
    </>
  );
}

