import { useEffect, useMemo, useState } from "react";
import {
  Bot,
  Brain,
  FolderOpen,
  LayoutDashboard,
  Pause,
  Plus,
  Search,
  Settings,
  Workflow,
} from "lucide-react";
import { Modal } from "./ui";
import { useUi } from "../stores/ui";
import { useProjects } from "../stores/projects";
import { useChat } from "../stores/chat";
import { searchChatLogs, type ChatLogRow } from "../lib/ipc";
import { cn } from "../lib/cn";
import { useTranslation } from "react-i18next";

interface CommandItem {
  id: string;
  category: "navigation" | "action" | "project" | "log";
  title: string;
  subtitle?: string;
  icon: any;
  action: () => void;
}

export function CommandPalette() {
  const { t } = useTranslation();
  const open = useUi((s) => s.commandPaletteOpen);
  const close = useUi((s) => s.closeCommandPalette);
  const { setWorkspaceTab, openSettings, openDashboard } = useUi();
  const { projects, activeSessionId, selectProject } = useProjects();
  const { cancel } = useChat();

  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [logResults, setLogResults] = useState<ChatLogRow[]>([]);
  const [searchingLogs, setSearchingLogs] = useState(false);

  // Debounced FTS log search when query has at least 2 characters
  useEffect(() => {
    if (!open || query.trim().length < 2) {
      setLogResults([]);
      return;
    }

    let active = true;
    setSearchingLogs(true);
    const timer = setTimeout(async () => {
      try {
        const results = await searchChatLogs(query.trim(), activeSessionId ?? undefined, 8);
        if (active) {
          setLogResults(results);
          setSearchingLogs(false);
        }
      } catch {
        if (active) setSearchingLogs(false);
      }
    }, 200);

    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [open, query, activeSessionId]);

  const baseCommands = useMemo<CommandItem[]>(() => {
    const list: CommandItem[] = [
      {
        id: "tab-chat",
        category: "navigation",
        title: t("palette.cmd.chat.title"),
        subtitle: t("palette.cmd.chat.subtitle"),
        icon: Bot,
        action: () => {
          setWorkspaceTab("chat");
          close();
        },
      },
      {
        id: "tab-plan",
        category: "navigation",
        title: t("palette.cmd.plan.title"),
        subtitle: t("palette.cmd.plan.subtitle"),
        icon: Brain,
        action: () => {
          setWorkspaceTab("plan");
          close();
        },
      },
      {
        id: "tab-pipeline",
        category: "navigation",
        title: t("palette.cmd.pipeline.title"),
        subtitle: t("palette.cmd.pipeline.subtitle"),
        icon: Workflow,
        action: () => {
          setWorkspaceTab("pipeline");
          close();
        },
      },
      {
        id: "nav-dashboard",
        category: "navigation",
        title: t("palette.cmd.dashboard.title"),
        subtitle: t("palette.cmd.dashboard.subtitle"),
        icon: LayoutDashboard,
        action: () => {
          openDashboard();
          close();
        },
      },
      {
        id: "nav-settings",
        category: "navigation",
        title: t("palette.cmd.settings.title"),
        subtitle: t("palette.cmd.settings.subtitle"),
        icon: Settings,
        action: () => {
          openSettings();
          close();
        },
      },
      {
        id: "act-new-demand",
        category: "action",
        title: t("palette.cmd.newRequest.title"),
        subtitle: t("palette.cmd.newRequest.subtitle"),
        icon: Plus,
        action: () => {
          setWorkspaceTab("plan");
          close();
          const textarea = document.querySelector("textarea");
          if (textarea) textarea.focus();
        },
      },
      {
        id: "act-pause-queue",
        category: "action",
        title: t("palette.cmd.stop.title"),
        subtitle: t("palette.cmd.stop.subtitle"),
        icon: Pause,
        action: () => {
          if (activeSessionId) cancel(activeSessionId);
          close();
        },
      },
    ];

    // Project switches
    projects.forEach((p) => {
      list.push({
        id: `proj-${p.id}`,
        category: "project",
        title: t("palette.switchProject", { name: p.name }),
        subtitle: p.path,
        icon: FolderOpen,
        action: () => {
          selectProject(p.id);
          close();
        },
      });
    });

    return list;
  }, [projects, activeSessionId, setWorkspaceTab, openDashboard, openSettings, selectProject, cancel, close, t]);

  const filteredCommands = useMemo(() => {
    const q = query.trim().toLowerCase();
    const matched = q
      ? baseCommands.filter(
          (c) =>
            c.title.toLowerCase().includes(q) ||
            (c.subtitle && c.subtitle.toLowerCase().includes(q))
        )
      : baseCommands;

    const logItems: CommandItem[] = logResults.map((l) => ({
      id: `log-${l.id}`,
      category: "log",
      title: t("palette.logHit", { role: l.role, p1: l.content.slice(0, 80).replace(/\n/g, " ") }),
      subtitle: `${l.attemptId} · ${l.createdAt.slice(11, 19)}`,
      icon: Search,
      action: () => {
        setWorkspaceTab("chat");
        close();
      },
    }));

    return [...matched, ...logItems];
  }, [baseCommands, logResults, query, setWorkspaceTab, close, t]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  // Keyboard navigation inside command palette
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((prev) => (prev + 1) % Math.max(1, filteredCommands.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((prev) => (prev - 1 + filteredCommands.length) % Math.max(1, filteredCommands.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (filteredCommands[selectedIndex]) {
        filteredCommands[selectedIndex].action();
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };

  if (!open) return null;

  return (
    <Modal
      open={open}
      onClose={close}
      className="max-w-xl p-0 overflow-hidden bg-elevated border-border shadow-2xl"
    >
      <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border bg-base/50">
        <Search className="size-4 text-muted shrink-0" />
        <input
          autoFocus
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={t("palette.placeholder")}
          className="flex-1 bg-transparent text-sm text-fg placeholder:text-muted outline-hidden"
        />
        <kbd className="hidden sm:inline-block px-1.5 py-0.5 text-[10px] font-mono text-muted bg-hover rounded border border-border">{t("palette.close")}</kbd>
      </div>

      <div className="max-h-80 overflow-y-auto p-1.5 divide-y divide-border/40">
        {filteredCommands.length === 0 ? (
          <div className="p-6 text-center text-xs text-muted">
            {searchingLogs ? t("palette.searching") : t("palette.noMatch")}
          </div>
        ) : (
          <div className="space-y-0.5">
            {filteredCommands.map((cmd, idx) => {
              const Icon = cmd.icon;
              const isSelected = idx === selectedIndex;
              return (
                <button
                  key={cmd.id}
                  type="button"
                  onClick={cmd.action}
                  onMouseEnter={() => setSelectedIndex(idx)}
                  className={cn(
                    "w-full flex items-center gap-3 px-3 py-2 rounded-lg text-left transition-colors",
                    isSelected ? "bg-accent text-accent-fg" : "text-fg hover:bg-hover/60"
                  )}
                >
                  <Icon
                    className={cn(
                      "size-4 shrink-0",
                      isSelected ? "text-accent-fg" : "text-muted"
                    )}
                  />
                  <div className="flex-1 min-w-0">
                    <div className="text-xs font-medium truncate">{cmd.title}</div>
                    {cmd.subtitle && (
                      <div
                        className={cn(
                          "text-[11px] truncate",
                          isSelected ? "text-accent-fg/80" : "text-muted"
                        )}
                      >
                        {cmd.subtitle}
                      </div>
                    )}
                  </div>
                  {cmd.category === "log" && (
                    <span
                      className={cn(
                        "text-[10px] font-mono px-1.5 py-0.5 rounded",
                        isSelected
                          ? "bg-accent-fg/20 text-accent-fg"
                          : "bg-muted/15 text-muted"
                      )}
                    >
                      FTS
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        )}
      </div>
    </Modal>
  );
}
