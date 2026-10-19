import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Loader2, Plus, Trash2 } from "lucide-react";
import { cn } from "../../lib/cn";
import {
  authorizeCommand,
  getCommandPolicy,
  inTauri,
  listApprovals,
  updateCommandRules,
  type ApprovalRecord,
  type AuthMode,
  type Authorization,
  type CommandLevel,
  type CommandPolicy,
  type ProviderInfo,
} from "../../lib/ipc";
import { authLayers, useAuth, supportsFullAuto } from "../../stores/auth";
import { sessionCwd, useProjects } from "../../stores/projects";
import { useProviders } from "../../stores/providers";
import { useSettings } from "../../stores/settings";
import { isProviderShown } from "./CliSettings";
import { FullAutoConfirmDialog } from "../FullAutoConfirmDialog";
import { ProviderLogo } from "../ProviderLogo";
import { Button, Card, Segmented, Select, SettingRow } from "../ui";

type StoredMode = Exclude<AuthMode, "full_auto">;
const STORED_MODES: StoredMode[] = ["ask_always", "ask_sensitive"];
const MODES: AuthMode[] = ["ask_always", "ask_sensitive", "full_auto"];
const INHERIT = "inherit";

/** Full auto supports either an outer sandbox or verified native workspace isolation. */
const sandboxed = supportsFullAuto;

/** Settings → Permissions & security (docs/10 §3.5, docs/11). */
export function SecuritySettings() {
  return (
    <div className="space-y-6">
      <ModeSettings />
      <Preview />
      <RuleSettings />
      <CliCapabilities />
      <ApprovalAuditLog />
    </div>
  );
}

function ModeSettings() {
  const { t } = useTranslation();
  const s = useSettings();
  const projects = useProjects((p) => p.projects);
  const activeSessionId = useProjects((p) => p.activeSessionId);
  const activeProject = useProjects((p) => p.projects.find((x) => x.sessions.some((y) => y.id === p.activeSessionId)));
  const session = activeProject?.sessions.find((x) => x.id === activeSessionId);
  const { sessionModes, setSessionMode, isFullAutoActive } = useAuth();
  const anySandboxed = useProviders((p) => p.providers.some(sandboxed));
  const [showFullAutoConfirm, setShowFullAutoConfirm] = useState(false);

  const isWindows =
    typeof navigator !== "undefined" &&
    (/win(dows|32|64)/i.test(navigator.userAgent) || (navigator as any).userAgentData?.platform === "Windows");
  const fullAutoAllowed = !isWindows && anySandboxed;

  const modeOptions = (inherited: string) => [
    { value: INHERIT, label: t("settings.security.inherit", { mode: inherited }) },
    ...STORED_MODES.map((m) => ({ value: m, label: t(`settings.security.modes.${m}`) })),
  ];

  const sessionModeOptions = (inherited: string) => [
    { value: INHERIT, label: t("settings.security.inherit", { mode: inherited }) },
    ...STORED_MODES.map((m) => ({ value: m, label: t(`settings.security.modes.${m}`) })),
    {
      value: "full_auto" as AuthMode,
      label: fullAutoAllowed
        ? t("settings.security.modes.full_auto")
        : `${t("settings.security.modes.full_auto")} (${isWindows ? t("settings.security.windowsUnsupported") : t("settings.security.fullAutoNoSandboxShort")})`,
      disabled: !fullAutoAllowed,
    },
  ];

  const label = (m: AuthMode) => t(`settings.security.modes.${m}`);

  return (
    <section aria-labelledby="auth-mode-title" className="space-y-2">
      <h2 id="auth-mode-title" className="px-1 text-xs font-medium text-muted">
        {t("settings.security.modeTitle")}
      </h2>
      <Card>
        <SettingRow title={t("settings.security.mode")} description={t(`settings.security.modeDesc.${s.authMode}`)}>
          <Segmented<AuthMode>
            label={t("settings.security.mode")}
            value={s.authMode}
            onChange={(v) => v !== "full_auto" && s.set("authMode", v)}
            options={STORED_MODES.map((m) => ({ value: m, label: label(m) }))}
          />
        </SettingRow>
        <div className="px-4 py-3.5 text-xs leading-relaxed text-muted">
          <span className="font-medium text-fg">{label("full_auto")}</span>
          {" · "}
          {isWindows
            ? t("settings.security.windowsUnsupported")
            : anySandboxed
              ? t("settings.security.fullAutoLater")
              : t("settings.security.fullAutoNoSandbox")}
        </div>
        <ModeTable />
      </Card>

      {projects.length > 0 && (
        <Card>
          <div className="px-4 pt-3.5 text-[13px] font-medium">{t("settings.security.projectTitle")}</div>
          <p className="px-4 pt-1 pb-1 text-xs text-muted">{t("settings.security.projectDesc")}</p>
          {projects.map((p) => (
            <SettingRow key={p.id} title={p.name} description={p.path}>
              <Select
                label={t("settings.security.projectMode", { name: p.name })}
                value={s.authProjectModes[p.id] ?? INHERIT}
                options={modeOptions(label(s.authMode))}
                onChange={(v) => {
                  const next = { ...s.authProjectModes };
                  if (v === INHERIT) delete next[p.id];
                  else next[p.id] = v as StoredMode;
                  s.set("authProjectModes", next);
                }}
              />
            </SettingRow>
          ))}
          {session && activeProject && (
            <SettingRow
              title={t("settings.security.sessionTitle", { title: session.title })}
              description={t("settings.security.sessionDesc")}
            >
              <Select
                label={t("settings.security.sessionTitle", { title: session.title })}
                value={isFullAutoActive(session.id) ? "full_auto" : (sessionModes[session.id] ?? INHERIT)}
                options={sessionModeOptions(label(s.authProjectModes[activeProject.id] ?? s.authMode))}
                onChange={(v) => {
                  if (v === "full_auto") {
                    setShowFullAutoConfirm(true);
                  } else {
                    setSessionMode(session.id, v === INHERIT ? null : (v as AuthMode));
                  }
                }}
              />
            </SettingRow>
          )}
        </Card>
      )}

      {showFullAutoConfirm && session && (
        <FullAutoConfirmDialog
          sessionId={session.id}
          onClose={() => setShowFullAutoConfirm(false)}
        />
      )}
    </section>
  );
}

type TableCell = "read_only" | "rest" | "routine" | "sensitive" | "all" | "none" | "forbidden";
const TABLE: Record<AuthMode, Record<"allow" | "ask" | "deny", TableCell>> = {
  ask_always: { allow: "read_only", ask: "rest", deny: "forbidden" },
  ask_sensitive: { allow: "routine", ask: "sensitive", deny: "forbidden" },
  full_auto: { allow: "all", ask: "none", deny: "forbidden" },
};

/** docs/11 §2 as a table. */
function ModeTable() {
  const { t } = useTranslation();
  return (
    <div className="overflow-x-auto px-4 py-3.5">
      <table className="w-full text-left text-xs">
        <caption className="sr-only">{t("settings.security.tableCaption")}</caption>
        <thead className="text-subtle">
          <tr>
            <th scope="col" className="pb-1.5 font-normal" />
            {(["allow", "ask", "deny"] as const).map((d) => (
              <th key={d} scope="col" className="pb-1.5 font-normal">
                {t(`settings.security.decisions.${d}`)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="text-muted">
          {MODES.map((m) => (
            <tr key={m} className="border-t border-border">
              <th scope="row" className="py-1.5 pr-3 font-medium text-fg">
                {t(`settings.security.modes.${m}`)}
              </th>
              {(["allow", "ask", "deny"] as const).map((d) => (
                <td key={d} className="py-1.5 pr-3">
                  {t(`settings.security.table.${TABLE[m][d]}`)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="mt-2 text-xs text-subtle">{t("settings.security.tableNote")}</p>
    </div>
  );
}

const DECISION_STYLE = { allow: "text-success", ask: "text-warning", deny: "text-danger" } as const;

/** Try a command or file edit against the lists and every mode. */
function Preview() {
  const { t } = useTranslation();
  const projects = useProjects((p) => p.projects);
  const activeProjectId = useProjects((p) => p.activeProjectId);
  const activeSessionId = useProjects((p) => p.activeSessionId);
  const [projectId, setProjectId] = useState(activeProjectId ?? projects[0]?.id ?? "");
  const [kind, setKind] = useState<"command" | "edit">("command");
  const [input, setInput] = useState("");
  const [result, setResult] = useState<Authorization | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const project = projects.find((p) => p.id === projectId);

  const run = async (e: FormEvent) => {
    e.preventDefault();
    if (!project || !input.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const sessionId = project.id === activeProjectId ? activeSessionId : null;
      const cwd = sessionCwd(project, sessionId);
      setResult(
        await authorizeCommand({
          command: input,
          edit: kind === "edit",
          cwd,
          workspaceRoots: [...new Set([cwd, project.path])],
          projectId: project.id,
          layers: authLayers(project.id, sessionId),
        }),
      );
    } catch (err) {
      setResult(null);
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-labelledby="auth-preview-title" className="space-y-2">
      <h2 id="auth-preview-title" className="px-1 text-xs font-medium text-muted">
        {t("settings.security.preview.title")}
      </h2>
      <Card>
        <form onSubmit={run} className="space-y-2 px-4 py-3.5">
          <p className="text-xs leading-relaxed text-muted">{t("settings.security.preview.desc")}</p>
          {!inTauri || projects.length === 0 ? (
            <p className="text-xs text-subtle">
              {t(inTauri ? "settings.security.preview.noProject" : "settings.security.preview.desktopOnly")}
            </p>
          ) : (
            <>
              <div className="flex flex-wrap gap-2">
                <Select
                  label={t("settings.security.preview.project")}
                  value={projectId}
                  options={projects.map((p) => ({ value: p.id, label: p.name }))}
                  onChange={(v) => {
                    setProjectId(v);
                    setResult(null);
                  }}
                />
                <Segmented<"command" | "edit">
                  label={t("settings.security.preview.kind")}
                  value={kind}
                  onChange={(v) => {
                    setKind(v);
                    setResult(null);
                  }}
                  options={[
                    { value: "command", label: t("settings.security.preview.command") },
                    { value: "edit", label: t("settings.security.preview.edit") },
                  ]}
                />
              </div>
              <div className="flex gap-2">
                <input
                  type="text"
                  spellCheck={false}
                  autoComplete="off"
                  aria-label={t(kind === "edit" ? "settings.security.preview.edit" : "settings.security.preview.command")}
                  placeholder={kind === "edit" ? "src/main.rs" : "rm -rf build && npm test"}
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  className="h-8 min-w-0 flex-1 rounded-md border border-border bg-elevated px-2 font-mono text-xs text-fg placeholder:text-subtle"
                />
                <Button type="submit" variant="primary" disabled={busy || !input.trim()}>
                  {busy && <Loader2 aria-hidden className="size-3.5 animate-spin" />}
                  {t("settings.security.preview.run")}
                </Button>
              </div>
            </>
          )}
          {error && (
            <p role="alert" className="text-xs break-words text-danger" data-selectable>
              {error}
            </p>
          )}
        </form>
        {result && <PreviewResult result={result} />}
      </Card>
    </section>
  );
}

function PreviewResult({ result }: { result: Authorization }) {
  const { t } = useTranslation();
  const { verdict, mode } = result;
  return (
    <div aria-live="polite" className="space-y-2 px-4 py-3.5 text-xs">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <span>
          <span className="text-subtle">{t("settings.security.preview.level")} </span>
          <span className="font-medium">{t(`settings.security.levels.${verdict.level}`)}</span>
        </span>
        {!verdict.resolved && <span className="text-warning">{t("settings.security.preview.unresolved")}</span>}
        {verdict.recoverable != null && (
          <span className="text-muted">
            {t(verdict.recoverable ? "settings.security.preview.recoverable" : "settings.security.preview.notRecoverable")}
          </span>
        )}
      </div>
      <ul className="grid grid-cols-3 gap-2">
        {MODES.map((m) => {
          const o = result.byMode[m];
          const current = mode.mode === m;
          return (
            <li
              key={m}
              className={cn("rounded-md border px-2 py-1.5", current ? "border-accent" : "border-border")}
              aria-current={current || undefined}
            >
              <div className="text-subtle">
                {t(`settings.security.modes.${m}`)}
                {current && ` · ${t(`settings.security.scopes.${mode.scope}`)}`}
              </div>
              <div className={cn("font-medium", DECISION_STYLE[o.decision])}>{t(`settings.security.decisions.${o.decision}`)}</div>
            </li>
          );
        })}
      </ul>
      {verdict.reasons.length > 0 && (
        <ul className="space-y-0.5 text-muted" data-selectable>
          {verdict.reasons.map((r, i) => (
            <li key={i} className="break-words">
              <span className={cn(r.level === "forbidden" && "text-danger", r.level === "sensitive" && "text-warning")}>
                {t(`settings.security.levels.${r.level}`)}
              </span>
              {" · "}
              <code className="font-mono">{r.code}</code>
              {r.detail && <code className="font-mono text-subtle"> {r.detail}</code>}
              {r.segment && <span className="text-subtle"> — {r.segment}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

const RULE_LEVELS: CommandLevel[] = ["read_only", "project", "sensitive", "forbidden"];

/** Local `command_rules` (global or per project), and the pack's lists read-only. */
function RuleSettings() {
  const { t } = useTranslation();
  const projects = useProjects((p) => p.projects);
  const [policy, setPolicy] = useState<CommandPolicy | null>(null);
  const [pattern, setPattern] = useState("");
  const [level, setLevel] = useState<CommandLevel>("project");
  const [scope, setScope] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [showPack, setShowPack] = useState(false);

  useEffect(() => {
    void getCommandPolicy().then(setPolicy, (e) => setError(String(e)));
  }, []);

  const change = async (c: Parameters<typeof updateCommandRules>[0]) => {
    setError(null);
    try {
      const rules = await updateCommandRules(c);
      setPolicy((p) => (p ? { ...p, rules } : p));
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    }
  };

  const add = async (e: FormEvent) => {
    e.preventDefault();
    if (!pattern.trim()) return;
    if (await change({ op: "upsert", projectId: scope || null, pattern, level })) setPattern("");
  };

  const projectName = (id?: string | null) =>
    id ? (projects.find((p) => p.id === id)?.name ?? id) : t("settings.security.rules.everyProject");

  return (
    <section aria-labelledby="auth-rules-title" className="space-y-2">
      <h2 id="auth-rules-title" className="px-1 text-xs font-medium text-muted">
        {t("settings.security.rules.title")}
      </h2>
      <Card>
        <form onSubmit={add} className="space-y-2 px-4 py-3.5">
          <p className="text-xs leading-relaxed text-muted">{t("settings.security.rules.desc")}</p>
          <div className="flex flex-wrap gap-2">
            <input
              type="text"
              spellCheck={false}
              autoComplete="off"
              aria-label={t("settings.security.rules.pattern")}
              placeholder="npm run lint"
              value={pattern}
              onChange={(e) => setPattern(e.target.value)}
              disabled={!inTauri}
              className="h-8 min-w-40 flex-1 rounded-md border border-border bg-elevated px-2 font-mono text-xs text-fg placeholder:text-subtle"
            />
            <Select<CommandLevel>
              label={t("settings.security.rules.level")}
              value={level}
              onChange={setLevel}
              options={RULE_LEVELS.map((l) => ({ value: l, label: t(`settings.security.levels.${l}`) }))}
            />
            <Select
              label={t("settings.security.rules.scope")}
              value={scope}
              onChange={setScope}
              options={[
                { value: "", label: t("settings.security.rules.everyProject") },
                ...projects.map((p) => ({ value: p.id, label: p.name })),
              ]}
            />
            <Button type="submit" disabled={!inTauri || !pattern.trim()}>
              <Plus aria-hidden className="size-3.5" />
              {t("settings.security.rules.add")}
            </Button>
          </div>
          {error && (
            <p role="alert" className="text-xs break-words text-danger" data-selectable>
              {error}
            </p>
          )}
        </form>
        {policy?.rules.length ? (
          <ul>
            {policy.rules.map((r) => (
              <li key={r.id} className="flex items-center gap-3 border-t border-border px-4 py-2 text-xs">
                <code className="min-w-0 flex-1 truncate font-mono" title={r.pattern} data-selectable>
                  {r.pattern}
                </code>
                <span className="text-muted">{t(`settings.security.levels.${r.level}`)}</span>
                <span className="w-28 truncate text-subtle">{projectName(r.projectId)}</span>
                <button
                  type="button"
                  aria-label={t("settings.security.rules.delete", { pattern: r.pattern })}
                  title={t("settings.security.rules.delete", { pattern: r.pattern })}
                  onClick={() => r.id != null && void change({ op: "delete", id: r.id })}
                  className="inline-flex size-6 items-center justify-center rounded-md text-muted hover:bg-hover hover:text-danger"
                >
                  <Trash2 aria-hidden className="size-3.5" />
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="px-4 py-3 text-xs text-subtle">{t("settings.security.rules.empty")}</p>
        )}
        <div>
          <button
            type="button"
            aria-expanded={showPack}
            onClick={() => setShowPack((v) => !v)}
            className="flex w-full items-center justify-between px-4 py-3 text-left text-xs text-muted hover:text-fg"
          >
            {t("settings.security.rules.pack")}
            <ChevronDown aria-hidden className={cn("size-3.5 transition-transform", showPack && "rotate-180")} />
          </button>
          {showPack && policy && (
            <dl className="space-y-3 px-4 pb-3.5 text-xs">
              {(["read_only", "sensitive", "forbidden", "forbidden_paths"] as const).map((k) => (
                <div key={k}>
                  <dt className="mb-1 text-subtle">
                    {t(`settings.security.packLists.${k}`)} ({policy.pack[k].length})
                  </dt>
                  <dd className="flex flex-wrap gap-1" data-selectable>
                    {policy.pack[k].map((p) => (
                      <code key={p} className="rounded bg-hover px-1.5 py-0.5 font-mono text-[11px] text-muted">
                        {p}
                      </code>
                    ))}
                  </dd>
                </div>
              ))}
            </dl>
          )}
        </div>
      </Card>
    </section>
  );
}

type Support = "full" | "partial" | "none";

/** Sandbox and credential isolation per CLI, from the adapter's capability bits (docs/11 §4.1, §6.2). */
function CliCapabilities() {
  const { t } = useTranslation();
  const providers = useProviders((p) => p.providers);
  const added = useSettings((s) => s.providerAdded);
  const shown = providers.filter((p) => p.adapter && isProviderShown(p, added));
  const support = (p: ProviderInfo): { sandbox: Support; credentials: Support } => {
    const caps = p.adapter?.caps ?? [];
    return {
      sandbox: caps.includes("OS_SANDBOX") ? "full" : caps.includes("OWN_SANDBOX") ? "partial" : "none",
      // Vendor variables are always stripped from the child env (§6.3); hiding the CLI's own
      // needs a shell wrapper and the sandbox.
      credentials: caps.includes("SHELL_WRAP") && caps.includes("OS_SANDBOX") ? "full" : "partial",
    };
  };
  const tone = { full: "text-success", partial: "text-warning", none: "text-subtle" } as const;

  return (
    <section aria-labelledby="auth-cli-title" className="space-y-2">
      <h2 id="auth-cli-title" className="px-1 text-xs font-medium text-muted">
        {t("settings.security.cli.title")}
      </h2>
      <Card>
        <p className="px-4 py-3 text-xs leading-relaxed text-muted">{t("settings.security.cli.desc")}</p>
        {shown.map((p) => {
          const s = support(p);
          return (
            <div key={p.id} className="flex items-center gap-3 px-4 py-2.5 text-xs">
              <ProviderLogo id={p.id} name={p.name} className="size-4" />
              <span className="min-w-0 flex-1 truncate text-[13px]">{p.name}</span>
              <span className="w-32">
                <span className="text-subtle">{t("settings.security.cli.sandbox")} </span>
                <span className={tone[s.sandbox]}>{t(`settings.security.cli.support.${s.sandbox}`)}</span>
              </span>
              <span className="w-32">
                <span className="text-subtle">{t("settings.security.cli.credentials")} </span>
                <span className={tone[s.credentials]}>{t(`settings.security.cli.support.${s.credentials}`)}</span>
              </span>
            </div>
          );
        })}
      </Card>
    </section>
  );
}

function ApprovalAuditLog() {
  const { t } = useTranslation();
  const [records, setRecords] = useState<ApprovalRecord[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    listApprovals()
      .then((recs) => {
        if (active) {
          setRecords(recs);
          setLoading(false);
        }
      })
      .catch(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  return (
    <section aria-labelledby="auth-audit-title" className="space-y-2">
      <h2 id="auth-audit-title" className="px-1 text-xs font-medium text-muted">
        {t("settings.security.audit.title")}
      </h2>
      <Card>
        <p className="px-4 py-3 text-xs leading-relaxed text-muted">{t("settings.security.audit.desc")}</p>
        {loading ? (
          <div className="flex items-center justify-center p-6 text-subtle">
            <Loader2 aria-hidden className="size-4 animate-spin" />
          </div>
        ) : records.length === 0 ? (
          <div className="px-4 py-4 text-center text-xs text-subtle">{t("settings.security.audit.empty")}</div>
        ) : (
          <div className="divide-y divide-border">
            {records.map((r, i) => (
              <div key={r.id ?? i} className="flex items-center gap-3 px-4 py-2.5 text-xs">
                <span
                  className={cn(
                    "rounded px-1.5 py-0.5 font-mono text-[11px] font-medium",
                    r.decision === "deny"
                      ? "bg-danger-soft text-danger"
                      : "bg-success-soft text-success",
                  )}
                >
                  {r.decision}
                </span>
                <code className="min-w-0 flex-1 truncate font-mono text-fg" title={r.command}>
                  {r.command}
                </code>
                <span className="shrink-0 text-subtle">{r.mode}</span>
                {r.decidedAt && (
                  <span className="shrink-0 text-subtle" title={r.decidedAt}>
                    {new Date(r.decidedAt).toLocaleTimeString()}
                  </span>
                )}
              </div>
            ))}
          </div>
        )}
      </Card>
    </section>
  );
}

