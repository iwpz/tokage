/**
 * F17.a: read-only list of CLI-native sessions (Claude / Codex / …) for the active project.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listNativeSessions, type NativeSession, type ProviderId } from "../lib/ipc";
import { useProjects } from "../stores/projects";
import { ProviderLogo } from "./ProviderLogo";

const REFRESH_MS = 30_000;

function formatUpdated(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  try {
    return new Date(t).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

function providerLabel(id: ProviderId): string {
  switch (id) {
    case "CLAUDE_CODE":
      return "Claude";
    case "CODEX_CLI":
      return "Codex";
    case "CURSOR_CLI":
      return "Cursor";
    case "COPILOT_CLI":
      return "Copilot";
    default:
      return id;
  }
}

export function ExternalSessionsSection({
  projectPath,
}: {
  /** When set, filter to this project cwd; otherwise list globally. */
  projectPath?: string | null;
}) {
  const { t } = useTranslation();
  const [sessions, setSessions] = useState<NativeSession[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const rows = await listNativeSessions(undefined, projectPath ?? undefined);
        if (!cancelled) setSessions(rows);
      } catch {
        if (!cancelled) setSessions([]);
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    void load();
    const id = window.setInterval(() => void load(), REFRESH_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [projectPath]);

  return (
    <div className="border-t border-border px-2 py-2" data-testid="external-sessions">
      <div className="flex h-7 items-center px-2">
        <span className="text-xs font-medium text-subtle">{t("sidebar.externalSessions")}</span>
      </div>
      {loading && sessions.length === 0 ? (
        <p className="px-2 py-1 text-xs text-subtle">…</p>
      ) : sessions.length === 0 ? (
        <p className="px-2 py-1 text-xs text-subtle">{t("sidebar.noExternalSessions")}</p>
      ) : (
        <ul className="space-y-0.5" aria-label={t("sidebar.externalSessions")}>
          {sessions.map((s) => (
            <li key={`${s.providerId}:${s.id}:${s.path}`}>
              <div
                title={t("sidebar.externalSessionReadOnly")}
                className="flex h-8 items-center gap-2 rounded-md px-2 text-[13px] text-muted"
              >
                <ProviderLogo id={s.providerId} name={providerLabel(s.providerId)} className="size-5" />
                <span className="min-w-0 flex-1 truncate" title={s.title}>
                  {s.title || s.id}
                </span>
                <span className="shrink-0 text-[10px] text-subtle">{formatUpdated(s.updatedAt)}</span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Sidebar helper: scopes list to the active project path when one is selected. */
export function ExternalSessionsForActiveProject() {
  const path = useProjects((s) => {
    const id = s.activeProjectId;
    return s.projects.find((p) => p.id === id)?.path ?? null;
  });
  return <ExternalSessionsSection projectPath={path} />;
}
