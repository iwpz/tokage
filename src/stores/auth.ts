import { create } from "zustand";
import type { AuthLayers, AuthMode, ProviderInfo } from "../lib/ipc";
import { useSettings } from "./settings";

/** Native workspace isolation is distinct from compatibility with Tokage's outer sandbox. */
export function supportsFullAuto(provider: ProviderInfo | undefined): boolean {
  const caps = provider?.adapter?.caps;
  return !!caps && (caps.includes("OS_SANDBOX") || caps.includes("NATIVE_WORKSPACE_SANDBOX"));
}

export type FullAutoTtl = "1h" | "today" | "session";

export interface FullAutoGrant {
  ttl: FullAutoTtl;
  grantedAt: number;
  expiresAt: number | null;
}

export function calculateFullAutoExpiry(ttl: FullAutoTtl, now: number = Date.now()): number | null {
  switch (ttl) {
    case "1h":
      return now + 60 * 60 * 1000;
    case "today": {
      const d = new Date(now);
      d.setHours(23, 59, 59, 999);
      return d.getTime();
    }
    case "session":
      return null;
  }
}

/**
 * Session-level authorization overrides (docs/11 §2). Temporary by design: kept in memory
 * only, so every session is back to its project's mode after a restart.
 */
interface AuthState {
  sessionModes: Record<string, AuthMode>;
  fullAutoGrants: Record<string, FullAutoGrant>;
  setSessionMode: (sessionId: string, mode: AuthMode | null) => void;
  grantFullAuto: (sessionId: string, ttl: FullAutoTtl) => void;
  revokeFullAuto: (sessionId: string) => void;
  isFullAutoActive: (sessionId: string) => boolean;
}

export const useAuth = create<AuthState>()((set, get) => ({
  sessionModes: {},
  fullAutoGrants: {},
  setSessionMode: (sessionId, mode) =>
    set((s) => {
      const sessionModes = { ...s.sessionModes };
      const fullAutoGrants = { ...s.fullAutoGrants };
      if (mode) {
        sessionModes[sessionId] = mode;
        if (mode !== "full_auto") {
          delete fullAutoGrants[sessionId];
        }
      } else {
        delete sessionModes[sessionId];
        delete fullAutoGrants[sessionId];
      }
      return { sessionModes, fullAutoGrants };
    }),
  grantFullAuto: (sessionId, ttl) => {
    const now = Date.now();
    const expiresAt = calculateFullAutoExpiry(ttl, now);
    set((s) => ({
      sessionModes: { ...s.sessionModes, [sessionId]: "full_auto" },
      fullAutoGrants: { ...s.fullAutoGrants, [sessionId]: { ttl, grantedAt: now, expiresAt } },
    }));
  },
  revokeFullAuto: (sessionId) => {
    set((s) => {
      const sessionModes = { ...s.sessionModes };
      delete sessionModes[sessionId];
      const fullAutoGrants = { ...s.fullAutoGrants };
      delete fullAutoGrants[sessionId];
      return { sessionModes, fullAutoGrants };
    });
  },
  isFullAutoActive: (sessionId) => {
    const s = get();
    if (s.sessionModes[sessionId] !== "full_auto") return false;
    const grant = s.fullAutoGrants[sessionId];
    if (grant?.expiresAt != null && Date.now() > grant.expiresAt) {
      // TTL expired: automatically downgrade to default / project mode
      s.revokeFullAuto(sessionId);
      return false;
    }
    return true;
  },
}));

/** Global → project → session, as `tokage_core::auth::Layers` takes them. */
export function authLayers(projectId: string | null, sessionId: string | null): AuthLayers {
  const s = useSettings.getState();
  let sessionMode = (sessionId && useAuth.getState().sessionModes[sessionId]) || null;
  if (sessionId && sessionMode === "full_auto" && !useAuth.getState().isFullAutoActive(sessionId)) {
    sessionMode = null;
  }
  return {
    global: s.authMode,
    project: (projectId && s.authProjectModes[projectId]) || null,
    session: sessionMode,
  };
}

/** Same resolution as `Layers::resolve`, for display; the backend's answer is authoritative. */
export function resolveMode(
  layers: AuthLayers,
  fullAutoAllowed: boolean,
  sessionId?: string | null,
): { mode: AuthMode; scope: keyof AuthLayers } {
  let resolvedLayers = layers;
  if (sessionId && layers.session === "full_auto" && !useAuth.getState().isFullAutoActive(sessionId)) {
    resolvedLayers = { ...layers, session: null };
  }
  for (const scope of ["session", "project"] as const) {
    const m = resolvedLayers[scope];
    if (m && (m !== "full_auto" || fullAutoAllowed)) return { mode: m, scope };
  }
  return { mode: resolvedLayers.global === "full_auto" ? "ask_sensitive" : resolvedLayers.global, scope: "global" };
}
