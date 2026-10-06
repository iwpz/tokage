import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Ban, Check, Copy, Info, Plus, RefreshCw, X } from "lucide-react";
import {
  inTauri,
  type Account,
  type Guidance,
  type InstallMethod,
  type ModelGroup,
  type ProviderInfo,
  type UsageWindow,
} from "../../lib/ipc";
import { cn } from "../../lib/cn";
import { isModelBlocked } from "../../lib/models";
import { useProviders } from "../../stores/providers";
import { useSettings } from "../../stores/settings";
import { CheckingBadge, StatusDot } from "../ProviderBadge";
import { ProviderLogo } from "../ProviderLogo";
import { SourceNotice } from "../SourceNotice";
import { Button, Card, IconButton, Switch } from "../ui";

/** Rescan button + last-scan time, rendered in the sticky settings header. */
export function CliSettingsActions() {
  const { t, i18n } = useTranslation();
  const { scanning, lastScan, refresh } = useProviders();
  return (
    <div className="flex items-center gap-3">
      <span className="text-xs text-subtle">
        {t("settings.cli.lastScan", {
          time: lastScan ? new Date(lastScan).toLocaleTimeString(i18n.language) : t("settings.cli.never"),
        })}
      </span>
      <Button onClick={() => void refresh()} disabled={scanning} className="shrink-0">
        <RefreshCw className={scanning ? "size-3.5 animate-spin" : "size-3.5"} />
        {scanning ? t("settings.cli.scanning") : t("settings.cli.refresh")}
      </Button>
    </div>
  );
}

/** Featured CLIs, CLIs the user added, and any other whitelisted CLI found on this machine. */
export function isProviderShown(p: ProviderInfo, added: string[]) {
  return p.featured || added.includes(p.id) || p.status !== "NOT_INSTALLED";
}

export function CliSettings() {
  const { t } = useTranslation();
  const providers = useProviders((s) => s.providers);
  const added = useSettings((s) => s.providerAdded);
  const shown = providers.filter((p) => isProviderShown(p, added));
  const addable = providers.filter((p) => !isProviderShown(p, added));

  return (
    <div className="space-y-4">
      <p className="text-[13px] leading-relaxed text-muted">{t("settings.cli.desc")}</p>
      {!inTauri && (
        <p role="note" className="rounded-lg bg-accent-soft px-3 py-2 text-xs text-fg">
          {t("settings.cli.browserMock")}
        </p>
      )}
      <div className="space-y-3">
        {shown.map((p) => (
          <ProviderCard key={p.id} provider={p} removable={added.includes(p.id) && p.status === "NOT_INSTALLED"} />
        ))}
      </div>
      {addable.length > 0 && <AddProvider options={addable} />}
    </div>
  );
}

/** Whitelist-only picker: users can add any supported CLI, never an arbitrary command. */
function AddProvider({ options }: { options: ProviderInfo[] }) {
  const { t } = useTranslation();
  const addProvider = useSettings((s) => s.addProvider);
  return (
    <div className="flex items-center gap-3 rounded-xl border border-dashed border-border px-4 py-3">
      <Plus aria-hidden className="size-4 text-muted" />
      <label htmlFor="add-cli" className="text-[13px] text-muted">
        {t("settings.cli.add")}
      </label>
      <select
        id="add-cli"
        value=""
        onChange={(e) => e.target.value && addProvider(e.target.value as ProviderInfo["id"])}
        className="h-8 min-w-48 rounded-md border border-border bg-elevated px-2 text-[13px] text-fg"
      >
        <option value="">{t("settings.cli.addPlaceholder")}</option>
        {options.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
      <span className="text-xs text-subtle">{t("settings.cli.addHint")}</span>
    </div>
  );
}

function ProviderCard({ provider: p, removable }: { provider: ProviderInfo; removable: boolean }) {
  const { t } = useTranslation();
  const enabled = useSettings((s) => s.providerEnabled[p.id] ?? true);
  const customPath = useSettings((s) => s.providerPaths[p.id] ?? "");
  const trusted = useSettings((s) => s.providerTrust[p.id] === true);
  const { setProviderEnabled, setProviderPath, removeProvider, setProviderTrust } = useSettings();
  const installed = p.status !== "NOT_INSTALLED";
  const checking = useProviders((s) => s.pending.includes(p.id));
  const scanning = useProviders((s) => s.scanning);
  const refreshOne = useProviders((s) => s.refreshOne);

  return (
    <Card>
      <div className="flex items-center gap-3 px-4 py-3.5">
        <ProviderLogo id={p.id} name={p.name} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-[13px] font-medium">{p.name}</span>
            <code className="font-mono text-xs text-subtle">{p.executable}</code>
          </div>
          <div className="mt-0.5 flex items-center gap-2">
            <StatusDot status={installed && !enabled ? "DISABLED" : p.status} withLabel />
            {checking && <CheckingBadge />}
          </div>
        </div>
        {installed && (
          <Switch label={`${t("settings.cli.enabled")}: ${p.name}`} checked={enabled} onChange={(v) => setProviderEnabled(p.id, v)} />
        )}
        {removable && (
          <IconButton label={`${t("settings.cli.remove")}: ${p.name}`} onClick={() => removeProvider(p.id)}>
            <X className="size-4" />
          </IconButton>
        )}
        <IconButton
          label={`${t("settings.cli.refreshOne")}: ${p.name}`}
          onClick={() => void refreshOne(p.id)}
          disabled={checking || scanning}
          className="disabled:pointer-events-none disabled:opacity-50"
        >
          <RefreshCw className={cn("size-4", checking && "animate-spin")} />
        </IconButton>
      </div>

      {installed && (
        <dl className="grid grid-cols-[6rem_1fr] gap-y-1.5 px-4 py-3 text-xs">
          <dt className="text-subtle">{t("settings.cli.version")}</dt>
          <dd className="font-mono" data-selectable>{p.version ?? "—"}</dd>
          <dt className="text-subtle">{t("settings.cli.path")}</dt>
          <dd className="truncate font-mono" data-selectable>{p.path ?? "—"}</dd>
          <dt className="text-subtle">{t("settings.cli.auth")}</dt>
          <dd data-selectable>
            <AccountLine authMethod={p.authMethod} account={p.account} />
          </dd>
          {p.adapter && (
            <>
              <dt className="text-subtle">{t("settings.cli.caps.label")}</dt>
              <dd>
                <div className="flex flex-wrap gap-1.5">
                  {p.adapter.caps.map((cap) => {
                    const isSecurity = [
                      "OS_SANDBOX",
                      "OWN_SANDBOX",
                      "READ_ONLY_MODE",
                      "WRITE_SCOPE",
                      "APPROVAL_CALLBACK",
                      "TOOL_RULES",
                      "DENIAL_EVENT",
                      "SHELL_WRAP",
                    ].includes(cap);
                    const label = t(`settings.cli.caps.${cap}`, cap);
                    const desc = t(`settings.cli.caps.${cap}_DESC`, "");
                    return (
                      <span
                        key={cap}
                        title={desc ? `${label} (${cap})\n${desc}` : cap}
                        className={cn(
                          "inline-flex cursor-help items-center gap-1 rounded-md border px-2 py-0.5 text-[11px] font-medium transition-colors",
                          isSecurity
                            ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                            : "border-border bg-subtle/10 text-fg hover:bg-hover",
                        )}
                      >
                        {isSecurity && <span className="size-1.5 rounded-full bg-emerald-500" />}
                        <span>{label}</span>
                      </span>
                    );
                  })}
                </div>
              </dd>
            </>
          )}
          {p.account?.usage && p.account.official && (
            <>
              <dt className="text-subtle">{t("settings.cli.usage.label")}</dt>
              <dd>
                <UsageWindows windows={p.account.usage.windows} observedAt={p.account.usage.observedAt} />
              </dd>
            </>
          )}
          <dt className="text-subtle">{t("settings.cli.models")}</dt>
          <dd>
            {p.modelGroups.length === 0 ? (
              "—"
            ) : (
              <>
                <ul className="flex flex-wrap gap-1">
                  {p.modelGroups.map((g) => (
                    <ModelChip key={g.key} group={g} />
                  ))}
                </ul>
                <p className="mt-1 text-subtle">
                  {[
                    p.modelsSource && t(`settings.cli.modelsSource.${p.modelsSource}`),
                    p.models.length > p.modelGroups.length &&
                      t("settings.cli.modelsMerged", { variants: p.models.length, groups: p.modelGroups.length }),
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              </>
            )}
          </dd>
        </dl>
      )}

      {p.guidance.map((g) => (
        <GuidanceRow key={g.code} guidance={g} values={{ version: p.version ?? "", versions: p.adapter?.versions ?? "" }} />
      ))}
      {p.status === "NOT_INSTALLED" && <InstallMethods methods={p.installMethods} />}

      {installed && p.chat && (
        <div className="px-4 py-3 text-xs">
          <div className="text-fg">{t("settings.cli.directLimit.label")}</div>
          <p className={cn("mt-0.5", p.directLimit ? "text-warning" : "text-subtle")} data-selectable>
            {p.directLimit
              ? t(`settings.cli.directLimit.${p.directLimit.code}`, {
                  detail: p.directLimit.code === "CURSOR_UNSAFE_PERMISSIONS" ? p.directLimit.detail.join(", ") : "",
                })
              : t("settings.cli.directLimit.none")}
          </p>
        </div>
      )}

      {installed && (
        <div className="flex items-start gap-3 px-4 py-3">
          <div className="min-w-0 flex-1 text-xs">
            <div className="text-fg">{t("settings.cli.trust.label")}</div>
            <p className="mt-0.5 text-subtle">{t("settings.cli.trust.desc")}</p>
            <p className="mt-0.5 text-subtle">
              {p.trustArgs.length > 0 ? (
                <>
                  {t("settings.cli.trust.flags", { args: "" })}
                  <code className="font-mono" data-selectable>{p.trustArgs.join(" ")}</code>
                </>
              ) : (
                t("settings.cli.trust.noFlags")
              )}
            </p>
          </div>
          <Switch
            label={`${t("settings.cli.trust.label")}: ${p.name}`}
            checked={trusted}
            onChange={(v) => setProviderTrust(p.id, v)}
          />
        </div>
      )}

      <div className="px-4 py-3">
        <label className="block text-xs text-subtle" htmlFor={`path-${p.id}`}>
          {t("settings.cli.customPath")}
        </label>
        <input
          id={`path-${p.id}`}
          value={customPath}
          onChange={(e) => setProviderPath(p.id, e.target.value)}
          placeholder={t("settings.cli.customPathPlaceholder")}
          spellCheck={false}
          className="mt-1.5 h-8 w-full rounded-md border border-border bg-bg px-2 font-mono text-xs text-fg placeholder:text-subtle"
        />
      </div>

      {installed && p.id === "CLAUDE_CODE" && (
        <ClaudeStatuslineGuide account={p.account} />
      )}
    </Card>
  );
}

const STATUSLINE_SH_SCRIPT = `#!/usr/bin/env bash
# Read Claude Code status line json payload from stdin
input=$(cat)
# Atomically cache for Tokage to inspect
echo "$input" > ~/.claude/tokage_statusline.json
# Fall back or print custom status if desired`;

const SETTINGS_JSON_SNIPPET = `{
  "statusLine": {
    "type": "command",
    "command": "bash ~/.claude/statusline.sh"
  }
}`;

function ClaudeStatuslineGuide({ account }: { account: Account | null }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const isCacheActive = account?.usage?.source === "STATUSLINE_CACHE";

  return (
    <div className="border-t border-border px-4 py-3 text-xs">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg">{t("settings.cli.statusline.title")}</span>
          <span
            className={cn(
              "inline-flex items-center rounded-full px-1.5 py-0.5 text-[10px] font-medium",
              isCacheActive ? "bg-emerald-500/10 text-emerald-500" : "bg-muted/10 text-subtle",
            )}
          >
            {isCacheActive ? t("settings.cli.statusline.active") : t("settings.cli.statusline.notConfigured")}
          </span>
        </div>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => setOpen(!open)}
          className="h-6 text-[11px] text-subtle hover:text-fg"
        >
          {open ? t("settings.cli.statusline.hideGuide") : t("settings.cli.statusline.showGuide")}
        </Button>
      </div>

      <p className="mt-1 text-subtle">{t("settings.cli.statusline.desc")}</p>

      {open && (
        <div className="mt-3 space-y-3 rounded-md bg-hover/50 p-3">
          <div>
            <div className="font-medium text-fg">{t("settings.cli.statusline.step1")}</div>
            <p className="mt-0.5 text-subtle">{t("settings.cli.statusline.step1Desc")}</p>
            <div className="mt-1.5">
              <CopyableCommand command={STATUSLINE_SH_SCRIPT} />
            </div>
          </div>

          <div>
            <div className="font-medium text-fg">{t("settings.cli.statusline.step2")}</div>
            <p className="mt-0.5 text-subtle">{t("settings.cli.statusline.step2Desc")}</p>
            <div className="mt-1.5">
              <CopyableCommand command={SETTINGS_JSON_SNIPPET} />
            </div>
          </div>

          <p className="text-[11px] text-muted">{t("settings.cli.statusline.note")}</p>
        </div>
      )}
    </div>
  );
}

/** Auth method plus billing channel: subscription / pay-as-you-go, plan, custom endpoint. */
function AccountLine({ authMethod, account: a }: { authMethod: string | null; account: Account | null }) {
  const { t } = useTranslation();
  if (!a) {
    if (!authMethod) return <>—</>;
    return (
      <span className="inline-flex items-center gap-1.5">
        <span>{authMethod}</span>
        <SourceNotice variant="popover" />
      </span>
    );
  }
  const parts = [
    authMethod,
    a.billing !== "UNKNOWN" && t(`settings.cli.billing.${a.billing}`),
    a.plan && t("settings.cli.plan", { plan: a.plan }),
  ].filter(Boolean);
  return (
    <span>
      <span className="inline-flex items-center gap-1.5">
        <span>{parts.join(" · ") || "—"}</span>
        <SourceNotice variant="popover" />
      </span>
      {!a.official && (
        <span className="mt-0.5 block text-subtle">
          {t("settings.cli.customEndpoint", { host: a.endpoint ?? "?" })}
        </span>
      )}
    </span>
  );
}

function windowLabel(minutes: number, t: (key: any, opts?: any) => string) {
  if (minutes === 10080) return t("settings.cli.usage.weekly");
  if (minutes % 1440 === 0) return t("settings.cli.usage.days", { n: minutes / 1440 });
  if (minutes % 60 === 0) return t("settings.cli.usage.hours", { n: minutes / 60 });
  return t("settings.cli.usage.minutes", { n: minutes });
}

function formatIn(seconds: number, t: (key: any, opts?: any) => string) {
  const m = Math.max(1, Math.round(seconds / 60));
  if (m < 60) return t("settings.cli.usage.inMinutes", { n: m });
  const h = Math.floor(m / 60);
  if (h < 48) return t("settings.cli.usage.inHours", { h, m: m % 60 });
  return t("settings.cli.usage.inDays", { d: Math.floor(h / 24), h: h % 24 });
}

/** Rolling quota windows as meters. A window whose reset time has passed shows as reset. */
function UsageWindows({ windows, observedAt }: { windows: UsageWindow[]; observedAt: string | null }) {
  const { t, i18n } = useTranslation();
  const now = Date.now() / 1000;
  return (
    <div className="min-w-0 space-y-1.5">
      {windows.map((w) => {
        const reset = w.resetsAt != null && w.resetsAt <= now;
        const used = reset ? 0 : Math.min(100, Math.max(0, w.usedPercent));
        const label = windowLabel(w.windowMinutes, t);
        const tone = used >= 90 ? "bg-danger" : used >= 70 ? "bg-warning" : "bg-accent";
        return (
          <div key={w.windowMinutes} className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
            <span className="w-12 shrink-0 text-muted">{label}</span>
            <div
              role="meter"
              aria-label={label}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={used}
              className="h-1.5 w-24 shrink-0 overflow-hidden rounded-full bg-hover"
            >
              <div className={cn("h-full rounded-full", tone)} style={{ width: `${used}%` }} />
            </div>
            <span className="w-9 shrink-0 text-right font-mono tabular-nums">{Math.round(used)}%</span>
            <span className="whitespace-nowrap text-subtle">
              {reset
                ? t("settings.cli.usage.resetSince")
                : w.resetsAt != null && t("settings.cli.usage.resets", { when: formatIn(w.resetsAt - now, t) })}
            </span>
          </div>
        );
      })}
      {observedAt && (
        <p className="text-subtle">
          {t("settings.cli.usage.observed", {
            time: new Date(observedAt).toLocaleString(i18n.language, {
              month: "short",
              day: "numeric",
              hour: "2-digit",
              minute: "2-digit",
            }),
          })}
        </p>
      )}
    </div>
  );
}

/**
 * One merged model, by name only. Models that merge several CLI ids get an info button
 * that opens a panel listing them; it stays open until closed so the ids can be copied.
 */
function ModelChip({ group: g }: { group: ModelGroup }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const blockedModels = useSettings((s) => s.blockedModels);
  const toggleBlockedModel = useSettings((s) => s.toggleBlockedModel);
  const ids = g.variants.map((v) => v.id);
  const multi = ids.length > 1;
  const panelId = `model-ids-${g.key}`;
  const isBlocked =
    isModelBlocked(g.key, blockedModels) ||
    isModelBlocked(g.name, blockedModels) ||
    ids.some((id) => isModelBlocked(id, blockedModels));

  return (
    <li
      className={cn(
        "relative inline-flex items-center gap-1 rounded py-0.5 pr-1 pl-1.5 text-[11px] transition-colors",
        isBlocked
          ? "border border-danger/30 bg-danger/10 text-danger opacity-75"
          : "bg-hover text-fg",
      )}
    >
      <span
        title={multi ? (isBlocked ? t("settings.planning.modelBlocked") : undefined) : ids[0]}
        className={cn(!multi && "pr-0.5", isBlocked && "line-through opacity-80")}
      >
        {g.name}
      </span>
      {multi && (
        <button
          type="button"
          aria-label={t("settings.cli.showIds", { name: g.name })}
          title={t("settings.cli.showIds", { name: g.name })}
          aria-expanded={open}
          aria-controls={panelId}
          onClick={() => setOpen((v) => !v)}
          className={cn(
            "inline-flex size-4 items-center justify-center rounded text-subtle hover:bg-active hover:text-fg",
            open && "bg-active text-fg",
          )}
        >
          <Info aria-hidden className="size-3" />
        </button>
      )}
      <button
        type="button"
        aria-label={isBlocked ? t("settings.planning.unblockModel") : t("settings.planning.blockModel")}
        title={isBlocked ? t("settings.planning.unblockModel") : t("settings.planning.blockModel")}
        onClick={() => toggleBlockedModel(g.key)}
        className={cn(
          "inline-flex size-4 items-center justify-center rounded transition-colors",
          isBlocked
            ? "text-danger hover:bg-danger/20"
            : "text-subtle hover:bg-active hover:text-danger",
        )}
      >
        <Ban aria-hidden className="size-3" />
      </button>
      {multi && open && (
        <div
          id={panelId}
          role="dialog"
          aria-label={t("settings.cli.modelIdsOf", { name: g.name })}
          className="absolute top-full left-0 z-20 mt-1 w-72 rounded-lg border border-border bg-elevated p-2 text-xs shadow-lg text-fg"
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              setOpen(false);
            }
          }}
        >
          <div className="mb-1.5 flex items-center gap-2">
            <span className="min-w-0 flex-1 truncate font-medium">{t("settings.cli.modelIdsOf", { name: g.name })}</span>
            <Button
              size="sm"
              variant="ghost"
              className="h-6 px-1.5"
              onClick={async () => {
                await navigator.clipboard.writeText(ids.join("\n"));
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              }}
            >
              {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
              {copied ? t("settings.cli.copied") : t("settings.cli.copy")}
            </Button>
            <IconButton label={t("settings.cli.close")} className="size-6" onClick={() => setOpen(false)}>
              <X className="size-3.5" />
            </IconButton>
          </div>
          <ul data-selectable className="max-h-56 overflow-y-auto rounded-md bg-hover px-2 py-1.5 font-mono leading-5 select-text">
            {ids.map((id) => (
              <li key={id}>{id}</li>
            ))}
          </ul>
        </div>
      )}
    </li>
  );
}

/** Install commands for this platform, one tab per method. Copy only; never executed. */
function InstallMethods({ methods }: { methods: InstallMethod[] }) {
  const { t } = useTranslation();
  const [index, setIndex] = useState(0);
  const current = methods[Math.min(index, methods.length - 1)];
  if (!current) return null;

  return (
    <div className="px-4 py-3">
      <div className="flex items-center gap-3">
        <span className="w-16 shrink-0 text-xs text-subtle">{t("settings.cli.install")}</span>
        <div role="tablist" aria-label={t("settings.cli.installVia")} className="flex flex-wrap gap-1">
          {methods.map((m, i) => (
            <button
              key={m.kind}
              type="button"
              role="tab"
              aria-selected={m === current}
              onClick={() => setIndex(i)}
              className={cn(
                "inline-flex h-6 items-center gap-1 rounded-md px-2 text-xs",
                m === current ? "bg-active text-fg" : "text-muted hover:bg-hover hover:text-fg",
                !m.available && m !== current && "opacity-60",
              )}
            >
              {t(`settings.cli.kinds.${m.kind}`)}
              {m.recommended && <span className="text-[10px] text-accent">★</span>}
            </button>
          ))}
        </div>
      </div>
      <div className="mt-2 flex items-start gap-3">
        <span className="w-16 shrink-0" />
        <div className="min-w-0 flex-1">
          <CopyableCommand command={current.command} />
          <p className="mt-1.5 text-xs text-subtle">
            {[
              current.recommended && t("settings.cli.recommended"),
              current.requires && t("settings.cli.requires", { what: current.requires }),
              !current.available && t("settings.cli.toolMissing", { tool: t(`settings.cli.kinds.${current.kind}`) }),
              t("settings.cli.copyOnly"),
            ]
              .filter(Boolean)
              .join(" · ")}
          </p>
        </div>
      </div>
    </div>
  );
}

function CopyableCommand({ command }: { command: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex items-start gap-2">
      <pre
        data-selectable
        className="min-w-0 flex-1 overflow-x-auto whitespace-pre rounded-md bg-hover px-2 py-1.5 font-mono text-xs"
      >
        {command}
      </pre>
      <Button
        size="sm"
        variant="ghost"
        onClick={async () => {
          await navigator.clipboard.writeText(command);
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        }}
      >
        {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
        {copied ? t("settings.cli.copied") : t("settings.cli.copy")}
      </Button>
    </div>
  );
}

const GUIDANCE_TONE: Record<Guidance["code"], "warn" | "info"> = {
  INSTALL: "info",
  LOGIN: "warn",
  SET_API_KEY: "warn",
  VERIFY_LOGIN: "info",
  THIRD_PARTY_BACKEND: "info",
  NO_MODELS: "warn",
  TIMEOUT: "warn",
  UNSUPPORTED_VERSION: "warn",
};

/** One "what to do next" line, with a copyable command when there is one. */
function GuidanceRow({ guidance: g, values }: { guidance: Guidance; values: { version: string; versions: string } }) {
  const { t } = useTranslation();
  const tone = GUIDANCE_TONE[g.code];
  const Icon = tone === "warn" ? AlertTriangle : Info;
  return (
    <div className="px-4 py-3">
      <p className={cn("flex items-start gap-2 text-xs", tone === "warn" ? "text-warning" : "text-muted")}>
        <Icon aria-hidden className="mt-px size-3.5 shrink-0" />
        <span>{t(`settings.cli.guidance.${g.code}`, values)}</span>
      </p>
      {g.command && (
        <div className="mt-2 pl-5.5">
          <CopyableCommand command={g.command} />
        </div>
      )}
    </div>
  );
}
