import { DEMO_PROJECT_ID } from "../lib/demoIdentity";
import { useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Check, ShieldCheck, X } from "lucide-react";
import { cn } from "../lib/cn";
import type { ProviderInfo } from "../lib/ipc";
import { useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";
import { useUi } from "../stores/ui";
import { Logo } from "./Logo";
import { ProviderLogo } from "./ProviderLogo";
import { isProviderShown } from "./settings/CliSettings";
import { Button } from "./ui";

/** Installed, enabled CLIs listed in the dialog. */
export function trustCandidates(
  providers: ProviderInfo[],
  enabled: Partial<Record<string, boolean>>,
  added: string[],
) {
  return providers.filter((p) => p.status !== "NOT_INSTALLED" && enabled[p.id] !== false && isProviderShown(p, added));
}

/** The flags as they'll actually run here, with the placeholder filled and spaces quoted. */
function trustCommand(args: string[], path: string) {
  const quoted = /[\s"'$]/.test(path) ? `'${path.replace(/'/g, "'\\''")}'` : path;
  return args.map((a) => (a === "<workspace>" ? quoted : a)).join(" ");
}

// Signatures already shown this app session, so closing the dialog doesn't reopen it at once.
const shown = new Set<string>();

/**
 * One dialog for the active workspace: Tokage asks for its own permission to run there,
 * and for each CLI whether it may receive that trust (a global choice, applied to every
 * workspace Tokage is allowed in). Opens by itself when something is unasked or the
 * workspace was denied; the composer reopens it when a send is blocked.
 */
export function TrustPrompt() {
  const { t } = useTranslation();
  const project = useProjects((s) => s.projects.find((p) => p.id === s.activeProjectId));
  const setProjectTrust = useProjects((s) => s.setProjectTrust);
  const providers = useProviders((s) => s.providers);
  const enabled = useSettings((s) => s.providerEnabled);
  const added = useSettings((s) => s.providerAdded);
  const trust = useSettings((s) => s.providerTrust);
  const setProviderTrust = useSettings((s) => s.setProviderTrust);
  const open = useUi((s) => s.trustPromptOpen);
  const { openTrustPrompt, closeTrustPrompt } = useUi();
  const dialog = useRef<HTMLDialogElement>(null);

  const clis = trustCandidates(providers, enabled, added);

  // Auto-open when the workspace or a CLI hasn't been decided, or the workspace was denied.
  const unasked = clis.filter((p) => trust[p.id] === undefined).map((p) => p.id);
  const isDemo = project?.id === DEMO_PROJECT_ID;
  const needs = project && !isDemo && (project.trusted !== true || unasked.length > 0);
  const signature = project ? `${project.id}:${project.trusted}:${unasked.join(",")}` : "";
  useEffect(() => {
    if (needs && !shown.has(signature)) {
      shown.add(signature);
      openTrustPrompt();
    }
  }, [needs, signature, openTrustPrompt]);

  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    if (open && project && !isDemo && !d.open) d.showModal();
    if ((!open || !project || isDemo) && d.open) d.close();
  }, [open, project, isDemo]);

  const setAll = (v: boolean) => {
    if (project) setProjectTrust(project.id, v);
    clis.forEach((p) => setProviderTrust(p.id, v));
  };

  return (
    <dialog
      ref={dialog}
      aria-labelledby="trust-title"
      aria-describedby="trust-intro"
      onCancel={closeTrustPrompt}
      // `close` fires asynchronously; ignore it if the dialog was reopened in the meantime.
      onClose={() => !dialog.current?.open && closeTrustPrompt()}
      className="m-auto w-[36rem] max-w-[calc(100vw-2rem)] rounded-xl border border-border bg-elevated p-0 text-fg shadow-xl backdrop:bg-black/40"
    >
      {project && (
        <div className="flex max-h-[85vh] flex-col">
          <div className="space-y-2 px-5 pt-5 pb-3">
            <h2 id="trust-title" className="flex items-center gap-2 text-[14px] font-medium">
              <ShieldCheck aria-hidden className="size-4 text-accent" />
              {t("settings.cli.trust.dialog.title", { name: project.name })}
            </h2>
            <p className="font-mono text-xs text-subtle" data-selectable>
              {project.path}
            </p>
            <p id="trust-intro" className="text-[13px] leading-relaxed text-muted">
              {t("settings.cli.trust.dialog.intro")}
            </p>
            <p className="text-xs leading-relaxed text-warning">{t("settings.cli.trust.dialog.scope")}</p>
          </div>

          <ul className="min-h-0 flex-1 divide-y divide-border overflow-y-auto border-y border-border">
            <TrustRow
              icon={<Logo className="size-9" />}
              name={t("settings.cli.trust.dialog.tokage")}
              detail={t("settings.cli.trust.dialog.tokageNote")}
              value={project.trusted}
              onChange={(v) => setProjectTrust(project.id, v)}
            />
            {clis.map((p) => (
              <TrustRow
                key={p.id}
                icon={<ProviderLogo id={p.id} name={p.name} />}
                name={p.name}
                command={p.trustArgs.length > 0 ? `${p.executable} ${trustCommand(p.trustArgs, project.path)}` : null}
                detail={p.trustArgs.length > 0 ? null : t("settings.cli.trust.dialog.noFlags")}
                value={trust[p.id]}
                onChange={(v) => setProviderTrust(p.id, v)}
              />
            ))}
          </ul>

          <div className="flex items-center gap-2 px-5 py-3">
            <Button onClick={() => setAll(false)}>{t("settings.cli.trust.dialog.denyAll")}</Button>
            <Button onClick={() => setAll(true)}>{t("settings.cli.trust.dialog.allowAll")}</Button>
            <span className="flex-1" />
            <Button variant="primary" onClick={closeTrustPrompt} autoFocus>
              {t("settings.cli.trust.dialog.done")}
            </Button>
          </div>
        </div>
      )}
    </dialog>
  );
}

/** One authorization line: who, the flags that will be passed, current state, deny / allow. */
function TrustRow({
  icon,
  name,
  command,
  detail,
  value,
  onChange,
}: {
  icon: ReactNode;
  name: string;
  command?: string | null;
  detail?: string | null;
  value: boolean | undefined;
  onChange: (v: boolean) => void;
}) {
  const { t } = useTranslation();
  const state = value === true ? "allowed" : value === false ? "denied" : "unasked";
  return (
    <li className="flex items-center gap-3 px-5 py-3">
      <span className="shrink-0">{icon}</span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-[13px] font-medium">{name}</span>
          <span
            className={cn(
              "rounded px-1.5 text-[10px]",
              state === "allowed" && "bg-accent-soft text-accent",
              state === "denied" && "bg-hover text-danger",
              state === "unasked" && "bg-hover text-subtle",
            )}
          >
            {t(`settings.cli.trust.dialog.state.${state}`)}
          </span>
        </div>
        {command && (
          <code data-selectable className="mt-1 block truncate font-mono text-[11px] text-muted" title={command}>
            {command}
          </code>
        )}
        {detail && <p className="mt-0.5 text-xs text-subtle">{detail}</p>}
      </div>
      <div role="group" aria-label={name} className="flex shrink-0 gap-1">
        <Button
          size="sm"
          variant={value === false ? "secondary" : "ghost"}
          aria-pressed={value === false}
          onClick={() => onChange(false)}
        >
          <X className="size-3.5" />
          {t("settings.cli.trust.dialog.deny")}
        </Button>
        <Button
          size="sm"
          variant={value === true ? "primary" : "ghost"}
          aria-pressed={value === true}
          onClick={() => onChange(true)}
        >
          <Check className="size-3.5" />
          {t("settings.cli.trust.dialog.allow")}
        </Button>
      </div>
    </li>
  );
}
