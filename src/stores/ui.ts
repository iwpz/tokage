import { create } from "zustand";

export type SettingsSection =
  | "general"
  | "cli"
  | "planning"
  | "context"
  | "budget"
  | "execution"
  | "security"
  | "proxy"
  | "knowledge"
  | "about";

export type WorkspaceTab = "chat" | "plan" | "pipeline" | "session";

/** What the right-hand preview pane shows. */
export interface Preview {
  /** Folder the file is read from (the turn's working directory). */
  root: string;
  /** Path relative to `root`, or absolute inside it. */
  path: string;
  /** Unified diff of the turn's change, when one is known. */
  diff: string | null;
  change: "added" | "modified" | "deleted" | null;
}

/** Pane width limits in px; the live window width tightens them further (see lib/layout). */
export const SIDEBAR_WIDTH = { min: 200, max: 480, default: 256 };
export const PREVIEW_WIDTH = { min: 300, max: 1200, default: 560 };

interface UiState {
  view: "workspace" | "settings" | "dashboard";
  settingsSection: SettingsSection;
  /** User's docked-sidebar preference on wide windows. */
  sidebarCollapsed: boolean;
  /** Narrow window: the sidebar becomes an overlay drawer that starts closed. */
  compact: boolean;
  drawerOpen: boolean;
  setCompact: (compact: boolean) => void;
  closeDrawer: () => void;
  sidebarWidth: number;
  previewWidth: number;
  setSidebarWidth: (w: number) => void;
  setPreviewWidth: (w: number) => void;
  workspaceTab: WorkspaceTab;
  /** Workspace authorization dialog (Tokage's permission plus per-CLI trust passing). */
  trustPromptOpen: boolean;
  openTrustPrompt: () => void;
  closeTrustPrompt: () => void;
  openSettings: (section?: SettingsSection) => void;
  closeSettings: () => void;
  openDashboard: () => void;
  closeDashboard: () => void;
  setSettingsSection: (section: SettingsSection) => void;
  toggleSidebar: () => void;
  setWorkspaceTab: (tab: WorkspaceTab) => void;
  preview: Preview | null;
  openPreview: (p: Preview) => void;
  closePreview: () => void;
}

export const useUi = create<UiState>()(
  (set) => ({
    view: "workspace",
    settingsSection: "general",
    sidebarCollapsed: false,
    compact: false,
    drawerOpen: false,
    setCompact: (compact) => set((s) => (s.compact === compact ? s : { compact, drawerOpen: false })),
    closeDrawer: () => set({ drawerOpen: false }),
    sidebarWidth: SIDEBAR_WIDTH.default,
    previewWidth: PREVIEW_WIDTH.default,
    setSidebarWidth: (w) => set({ sidebarWidth: Math.round(w) }),
    setPreviewWidth: (w) => set({ previewWidth: Math.round(w) }),
    workspaceTab: "chat",
    trustPromptOpen: false,
    openTrustPrompt: () => set({ trustPromptOpen: true }),
    closeTrustPrompt: () => set({ trustPromptOpen: false }),
    openSettings: (section) =>
      set((s) => ({ view: "settings", settingsSection: section ?? s.settingsSection })),
    closeSettings: () => set({ view: "workspace" }),
    openDashboard: () => set({ view: "dashboard" }),
    closeDashboard: () => set({ view: "workspace" }),
    setSettingsSection: (settingsSection) => set({ settingsSection }),
    toggleSidebar: () =>
      set((s) => (s.compact ? { drawerOpen: !s.drawerOpen } : { sidebarCollapsed: !s.sidebarCollapsed })),
    setWorkspaceTab: (workspaceTab) => set({ workspaceTab }),
    preview: null,
    openPreview: (preview) => set({ preview }),
    closePreview: () => set({ preview: null }),
  }),
);

/** The part of the UI state that is kept across launches. */
export const UI_KEPT = ["sidebarCollapsed", "sidebarWidth", "previewWidth"] as const;
