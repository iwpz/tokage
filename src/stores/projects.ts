import { create } from "zustand";

export interface SessionWorktree {
  /** Worktree checkout root. */
  root: string;
  /** Where the session's turns run (the project's folder inside the worktree). */
  cwd: string;
  branch: string;
}

export interface MainSession {
  id: string;
  title: string;
  state: "draft" | "review" | "running" | "done" | "blocked";
  updatedAt: number;
  /** Set when the session works in its own git worktree instead of the project folder. */
  worktree?: SessionWorktree;
  /** Hidden from the main list; restorable. The worktree, if any, is kept. */
  archived?: boolean;
  /** Session this one was forked from. */
  forkedFrom?: string;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  expanded: boolean;
  sessions: MainSession[];
  /** Tokage's own permission to run tasks here. Unset = not asked yet. */
  trusted?: boolean;
}

interface ProjectsState {
  projects: Project[];
  activeProjectId: string | null;
  activeSessionId: string | null;
  addProject: (path: string) => void;
  removeProject: (id: string) => void;
  toggleProject: (id: string) => void;
  selectProject: (id: string) => void;
  selectSession: (projectId: string, sessionId: string) => void;
  /** Creates a session under the project, makes it active, and returns its id. */
  createSession: (projectId: string, title: string, worktree?: SessionWorktree) => string;
  touchSession: (sessionId: string) => void;
  renameSession: (sessionId: string, title: string) => void;
  setSessionArchived: (sessionId: string, archived: boolean) => void;
  /** Drops the session record only; callers clean up its conversation and worktree. */
  deleteSession: (sessionId: string) => void;
  /** New active session right after the source, linked to it. Returns its id. */
  forkSession: (sourceId: string, title: string, worktree?: SessionWorktree) => string | null;
  setProjectTrust: (id: string, trusted: boolean) => void;
}

const uid = () => crypto.randomUUID();

/** Where a session's turns run: its worktree, or the project folder. */
export function sessionCwd(project: Project, sessionId: string | null): string {
  return project.sessions.find((s) => s.id === sessionId)?.worktree?.cwd ?? project.path;
}

const nameFromPath = (path: string) => {
  const parts = path.replace(/[\\/]+$/, "").split(/[\\/]/);
  return parts[parts.length - 1] || path;
};

export const useProjects = create<ProjectsState>()(
  (set, get) => ({
    projects: [],
    activeProjectId: null,
    activeSessionId: null,

    addProject: (path) =>
      set((s) => {
        const existing = s.projects.find((p) => p.path === path);
        if (existing) return { activeProjectId: existing.id, activeSessionId: null };
        const project: Project = {
          id: uid(),
          name: nameFromPath(path),
          path,
          expanded: true,
          sessions: [],
        };
        return {
          projects: [...s.projects, project],
          activeProjectId: project.id,
          activeSessionId: null,
        };
      }),

    removeProject: (id) =>
      set((s) => {
        const projects = s.projects.filter((p) => p.id !== id);
        const wasActive = s.activeProjectId === id;
        return {
          projects,
          activeProjectId: wasActive ? (projects[0]?.id ?? null) : s.activeProjectId,
          activeSessionId: wasActive ? null : s.activeSessionId,
        };
      }),

    toggleProject: (id) =>
      set((s) => ({
        projects: s.projects.map((p) => (p.id === id ? { ...p, expanded: !p.expanded } : p)),
      })),

    selectProject: (id) => set({ activeProjectId: id, activeSessionId: null }),

    selectSession: (projectId, sessionId) =>
      set({ activeProjectId: projectId, activeSessionId: sessionId }),

    createSession: (projectId, title, worktree) => {
      const session: MainSession = { id: uid(), title, state: "draft", updatedAt: Date.now(), worktree };
      set((s) => ({
        projects: s.projects.map((p) =>
          p.id === projectId ? { ...p, expanded: true, sessions: [session, ...p.sessions] } : p,
        ),
        activeProjectId: projectId,
        activeSessionId: session.id,
      }));
      return session.id;
    },

    renameSession: (sessionId, title) =>
      set((s) => ({
        projects: s.projects.map((p) => ({
          ...p,
          sessions: p.sessions.map((x) => (x.id === sessionId ? { ...x, title } : x)),
        })),
      })),

    setSessionArchived: (sessionId, archived) =>
      set((s) => ({
        projects: s.projects.map((p) => ({
          ...p,
          sessions: p.sessions.map((x) => (x.id === sessionId ? { ...x, archived } : x)),
        })),
        // An archived session leaves the view; the project's start page takes its place.
        activeSessionId: archived && s.activeSessionId === sessionId ? null : s.activeSessionId,
      })),

    deleteSession: (sessionId) =>
      set((s) => ({
        projects: s.projects.map((p) => ({ ...p, sessions: p.sessions.filter((x) => x.id !== sessionId) })),
        activeSessionId: s.activeSessionId === sessionId ? null : s.activeSessionId,
      })),

    forkSession: (sourceId, title, worktree) => {
      const project = get().projects.find((p) => p.sessions.some((x) => x.id === sourceId));
      if (!project) return null;
      const session: MainSession = {
        id: uid(),
        title,
        state: "draft",
        updatedAt: Date.now(),
        worktree,
        forkedFrom: sourceId,
      };
      set((s) => ({
        projects: s.projects.map((p) => {
          if (p.id !== project.id) return p;
          const i = p.sessions.findIndex((x) => x.id === sourceId);
          const sessions = [...p.sessions];
          sessions.splice(i + 1, 0, session);
          return { ...p, expanded: true, sessions };
        }),
        activeProjectId: project.id,
        activeSessionId: session.id,
      }));
      return session.id;
    },

    setProjectTrust: (id, trusted) =>
      set((s) => ({ projects: s.projects.map((p) => (p.id === id ? { ...p, trusted } : p)) })),

    touchSession: (sessionId) =>
      set((s) => ({
        projects: s.projects.map((p) => {
          const i = p.sessions.findIndex((x) => x.id === sessionId);
          if (i < 0) return p;
          const updated = { ...p.sessions[i], updatedAt: Date.now() };
          return { ...p, sessions: [updated, ...p.sessions.filter((_, j) => j !== i)] };
        }),
      })),
  }),
);
