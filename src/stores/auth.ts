import { create } from "zustand";
import type { AuthLayers, AuthMode } from "../lib/ipc";
import { useSettings } from "./settings";

/**
 * Session-level authorization overrides (docs/11 §2). Temporary by design: kept in memory
 * only, so every session is back to its project's mode after a restart.
 */
interface AuthState {
  sessionModes: Record<string, AuthMode>;
  setSessionMode: (sessionId: string, mode: AuthMode | null) => void;
}

export const useAuth = create<AuthState>()((set) => ({
  sessionModes: {},
  setSessionMode: (sessionId, mode) =>
    set((s) => {
      const sessionModes = { ...s.sessionModes };
      if (mode) sessionModes[sessionId] = mode;
      else delete sessionModes[sessionId];
      return { sessionModes };
    }),
}));

/** Global → project → session, as `tokage_core::auth::Layers` takes them. */
export function authLayers(projectId: string | null, sessionId: string | null): AuthLayers {
  const s = useSettings.getState();
  return {
    global: s.authMode,
    project: (projectId && s.authProjectModes[projectId]) || null,
    session: (sessionId && useAuth.getState().sessionModes[sessionId]) || null,
  };
}

/** Same resolution as `Layers::resolve`, for display; the backend's answer is authoritative. */
export function resolveMode(layers: AuthLayers, fullAutoAllowed: boolean): { mode: AuthMode; scope: keyof AuthLayers } {
  for (const scope of ["session", "project"] as const) {
    const m = layers[scope];
    if (m && (m !== "full_auto" || fullAutoAllowed)) return { mode: m, scope };
  }
  return { mode: layers.global === "full_auto" ? "ask_sensitive" : layers.global, scope: "global" };
}
