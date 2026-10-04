import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, ChevronDown, Download, Loader2, RotateCcw, ShieldAlert } from "lucide-react";
import { cn } from "../../lib/cn";
import type { ProviderId } from "../../lib/ipc";
import { parsePack, type PackModel, type PackOffering } from "../../lib/knowledge";
import builtinPackText from "../../../src-tauri/resources/knowledge/builtin.json?raw";
import { useKnowledge } from "../../stores/knowledge";
import { useProviders } from "../../stores/providers";
import { useSettings } from "../../stores/settings";
import { Button, Card, SettingRow, Switch } from "../ui";

/** The pack shipped with Tokage (docs/04 knowledge-pack §6, `builtin` layer). */
const builtin = parsePack(builtinPackText);

export function KnowledgeSettings() {
  const { t } = useTranslation();
  const s = useSettings();
  const { url, custom, loading, error, setUrl, load, resetToBuiltin } = useKnowledge();
  const [confirming, setConfirming] = useState(false);

  const trimmed = url.trim();
  const valid = /^https:\/\/\S+$/i.test(trimmed) || /^http:\/\/(localhost|127\.0\.0\.1|\[::1\])([:/?#]\S*)?$/i.test(trimmed);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (valid && !loading) setConfirming(true);
  };

  return (
    <div className="space-y-6">
      <Card>
        <SettingRow title={t("settings.knowledge.version")}>
          <span className="flex items-center gap-2">
            {custom && (
              <span className="rounded bg-hover px-1.5 text-[10px] text-warning">{t("settings.knowledge.thirdParty")}</span>
            )}
            <code className="font-mono text-xs text-muted">{custom ? custom.pack.packVersion : builtin.pack.packVersion}</code>
          </span>
        </SettingRow>
        <SettingRow title={t("settings.knowledge.autoUpdate")} description={t("settings.knowledge.autoUpdateDesc")}>
          <Switch label={t("settings.knowledge.autoUpdate")} checked={s.knowledgeAutoUpdate} onChange={(v) => s.set("knowledgeAutoUpdate", v)} />
        </SettingRow>
        <SettingRow title={t("settings.knowledge.check")}>
          <Button disabled title={t("common.comingSoon")}>
            {t("settings.knowledge.check")}
          </Button>
        </SettingRow>
      </Card>

      <section aria-labelledby="pack-url-title" className="space-y-2">
        <h2 id="pack-url-title" className="px-1 text-xs font-medium text-muted">
          {t("settings.knowledge.custom.title")}
        </h2>
        <Card>
          <form onSubmit={submit} className="space-y-2 px-4 py-3.5">
            <label htmlFor="pack-url" className="block text-xs leading-relaxed text-muted">
              {t("settings.knowledge.custom.desc")}
            </label>
            <div className="flex gap-2">
              <input
                id="pack-url"
                type="url"
                inputMode="url"
                spellCheck={false}
                autoComplete="off"
                placeholder="https://example.com/tokage-pack.json"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                aria-invalid={trimmed !== "" && !valid}
                aria-describedby={error ? "pack-url-error" : undefined}
                className="h-8 min-w-0 flex-1 rounded-md border border-border bg-elevated px-2 font-mono text-xs text-fg placeholder:text-subtle"
              />
              <Button type="submit" variant="primary" disabled={!valid || loading}>
                {loading ? <Loader2 aria-hidden className="size-3.5 animate-spin" /> : <Download aria-hidden className="size-3.5" />}
                {custom && custom.url === trimmed ? t("settings.knowledge.custom.update") : t("settings.knowledge.custom.load")}
              </Button>
            </div>
            {trimmed !== "" && !valid && <p className="text-xs text-danger">{t("settings.knowledge.custom.invalidUrl")}</p>}
            {error && (
              <p id="pack-url-error" role="alert" className="text-xs break-words text-danger" data-selectable>
                {t("settings.knowledge.custom.failed", { error })}
              </p>
            )}
          </form>

          {custom && (
            <div className="flex items-start gap-3 bg-hover/50 px-4 py-3">
              <ShieldAlert aria-hidden className="mt-0.5 size-4 shrink-0 text-warning" />
              <div className="min-w-0 flex-1 text-xs leading-relaxed">
                <p className="text-fg">{t("settings.knowledge.custom.active")}</p>
                <p className="truncate font-mono text-muted" title={custom.url} data-selectable>
                  {custom.url}
                </p>
                <p className="text-subtle">
                  {t("settings.knowledge.custom.loadedAt", { time: new Date(custom.loadedAt).toLocaleString() })}
                </p>
              </div>
              <Button size="sm" onClick={resetToBuiltin}>
                <RotateCcw aria-hidden className="size-3.5" />
                {t("settings.knowledge.custom.reset")}
              </Button>
            </div>
          )}
        </Card>
      </section>

      {custom ? (
        <PackModels models={custom.pack.models} domains={custom.pack.domains} warnings={custom.warnings} />
      ) : (
        <PackModels models={builtin.pack.models} domains={builtin.pack.domains} warnings={builtin.warnings} />
      )}

      <ThirdPartyDialog
        open={confirming}
        url={trimmed}
        onCancel={() => setConfirming(false)}
        onConfirm={() => {
          setConfirming(false);
          void load(trimmed);
        }}
      />
    </div>
  );
}

/** Shown before every load from a custom URL: Tokage can't vouch for what the pack contains. */
function ThirdPartyDialog({ open, url, onCancel, onConfirm }: { open: boolean; url: string; onCancel: () => void; onConfirm: () => void }) {
  const { t } = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const [ack, setAck] = useState(false);

  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    if (open && !d.open) {
      setAck(false);
      d.showModal();
    }
    if (!open && d.open) d.close();
  }, [open]);

  return (
    <dialog
      ref={dialog}
      aria-labelledby="pack-warn-title"
      aria-describedby="pack-warn-body"
      onCancel={onCancel}
      onClose={() => !dialog.current?.open && open && onCancel()}
      className="m-auto w-[32rem] max-w-[calc(100vw-2rem)] rounded-xl border border-border bg-elevated p-0 text-fg shadow-xl backdrop:bg-black/40"
    >
      <div className="space-y-3 px-5 pt-5 pb-4">
        <h2 id="pack-warn-title" className="flex items-center gap-2 text-[14px] font-medium">
          <AlertTriangle aria-hidden className="size-4 text-warning" />
          {t("settings.knowledge.warn.title")}
        </h2>
        <p className="font-mono text-xs break-all text-subtle" data-selectable>
          {url}
        </p>
        <div id="pack-warn-body" className="space-y-2 text-[13px] leading-relaxed text-muted">
          <p>{t("settings.knowledge.warn.body")}</p>
          <ul className="list-disc space-y-1 pl-5 text-xs">
            <li>{t("settings.knowledge.warn.risk1")}</li>
            <li>{t("settings.knowledge.warn.risk2")}</li>
            <li>{t("settings.knowledge.warn.risk3")}</li>
          </ul>
          <p className="text-xs">{t("settings.knowledge.warn.advice")}</p>
        </div>
        <label className="flex items-start gap-2 text-xs text-fg">
          <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} className="mt-0.5 accent-[var(--accent)]" />
          {t("settings.knowledge.warn.ack")}
        </label>
      </div>
      <div className="flex justify-end gap-2 border-t border-border px-5 py-3">
        <Button onClick={onCancel} autoFocus>
          {t("common.cancel")}
        </Button>
        <Button variant="primary" disabled={!ack} onClick={onConfirm}>
          {t("settings.knowledge.warn.confirm")}
        </Button>
      </div>
    </dialog>
  );
}

function PackModels({ models, domains, warnings }: { models: PackModel[]; domains: string[]; warnings: string[] }) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q
      ? models.filter((m) =>
          [m.displayName, m.modelId, m.vendor ?? "", ...m.tags, ...m.offerings.flatMap((o) => [o.provider, o.cliModelArg])].some((x) =>
            x.toLowerCase().includes(q),
          ),
        )
      : models;
  }, [models, query]);

  return (
    <section aria-labelledby="pack-models-title" className="space-y-2">
      <div className="flex items-center justify-between gap-3 px-1">
        <h2 id="pack-models-title" className="text-xs font-medium text-muted">
          {t("settings.knowledge.models.title", { count: models.length })}
        </h2>
        {models.length > 5 && (
          <input
            type="search"
            aria-label={t("settings.knowledge.models.filter")}
            placeholder={t("settings.knowledge.models.filter")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="h-7 w-48 rounded-md border border-border bg-elevated px-2 text-xs text-fg placeholder:text-subtle"
          />
        )}
      </div>

      {warnings.length > 0 && (
        <details className="rounded-xl border border-border bg-elevated text-xs">
          <summary className="flex cursor-pointer items-center gap-2 px-4 py-2.5 text-warning">
            <AlertTriangle aria-hidden className="size-3.5" />
            {t("settings.knowledge.models.warnings", { count: warnings.length })}
          </summary>
          <ul className="max-h-48 space-y-0.5 overflow-y-auto border-t border-border px-4 py-2 font-mono text-[11px] text-muted" data-selectable>
            {warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </details>
      )}

      {models.length === 0 ? (
        <Card>
          <p className="px-4 py-6 text-center text-xs text-subtle">{t("settings.knowledge.models.empty")}</p>
        </Card>
      ) : (
        <Card>
          {shown.map((m) => (
            <ModelRow key={m.modelId} model={m} domains={domains} />
          ))}
          {shown.length === 0 && <p className="px-4 py-6 text-center text-xs text-subtle">{t("settings.knowledge.models.noMatch")}</p>}
        </Card>
      )}
    </section>
  );
}

const fmtTokens = (n: number) => (n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${Math.round(n / 1000)}K` : String(n));

function ModelRow({ model: m, domains }: { model: PackModel; domains: string[] }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  // Pack domains first, then any extra the model scores.
  const keys = [...domains.filter((d) => d in m.skills), ...Object.keys(m.skills).filter((d) => !domains.includes(d))];
  const top = [...keys].sort((a, b) => m.skills[b] - m.skills[a]).slice(0, 3);
  const facts = [
    m.contextWindow && t("settings.knowledge.models.context", { value: fmtTokens(m.contextWindow) }),
    m.effectiveContext && t("settings.knowledge.models.effective", { value: fmtTokens(m.effectiveContext) }),
    m.price && t("settings.knowledge.models.price", { input: m.price.input, output: m.price.output }),
    m.speed !== null && t("settings.knowledge.models.speed", { value: m.speed }),
  ].filter((x): x is string => !!x);
  const id = `pack-model-${m.modelId}`.replace(/[^\w-]/g, "_");
  const providerName = useProviderName();

  return (
    <div>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-hover/50"
      >
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="text-[13px] font-medium text-fg">{m.displayName}</span>
            <code className="font-mono text-[11px] text-subtle">{m.modelId}</code>
            {m.offerings.length > 0 && (
              <span className="rounded bg-hover px-1.5 text-[10px] text-muted">
                {t("settings.knowledge.models.providers", { count: m.offerings.length })}
              </span>
            )}
            {m.roles.map((r) => (
              <span key={r} className="rounded bg-accent-soft px-1.5 text-[10px] text-accent">
                {r}
              </span>
            ))}
          </div>
          {!open && (top.length > 0 || m.offerings.length > 0) && (
            <div className="mt-1 truncate text-xs text-muted">
              {[top.map((d) => `${d} ${m.skills[d]}`).join(" · "), m.offerings.map((o) => providerName(o.provider)).join(", ")]
                .filter(Boolean)
                .join("  —  ")}
            </div>
          )}
        </div>
        <ChevronDown aria-hidden className={cn("size-4 shrink-0 text-subtle transition-transform", open && "rotate-180")} />
      </button>

      {open && (
        <div id={id} className="space-y-3 px-4 pb-4">
          {m.description && <p className="text-xs leading-relaxed text-muted">{m.description}</p>}
          {facts.length > 0 && (
            <p className="text-[11px] text-subtle">
              {facts.join(" · ")}
              {(m.priceEstimated || m.skillsEstimated) && (
                <span className="ml-2 rounded bg-hover px-1 text-warning">
                  {t(m.skillsEstimated ? "settings.knowledge.models.skillsEstimated" : "settings.knowledge.models.priceEstimated")}
                </span>
              )}
            </p>
          )}
          {keys.length > 0 ? (
            <dl className="grid grid-cols-1 gap-x-6 gap-y-1.5 sm:grid-cols-2">
              {keys.map((d) => (
                <SkillBar key={d} domain={d} score={m.skills[d]} />
              ))}
            </dl>
          ) : (
            <p className="text-xs text-subtle">{t("settings.knowledge.models.noSkills")}</p>
          )}
          {m.offerings.length > 0 && <Offerings model={m} providerName={providerName} />}
          {m.tags.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {m.tags.map((tag) => (
                <span key={tag} className="rounded border border-border px-1.5 text-[10px] text-muted">
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Names for CLIs not in the last scan (not added yet); mirrors REGISTRY in src-tauri/src/providers.rs. */
const CLI_NAMES: Record<ProviderId, string> = {
  CLAUDE_CODE: "Claude Code",
  CODEX_CLI: "Codex CLI",
  CURSOR_CLI: "Cursor CLI",
  ANTIGRAVITY_CLI: "Antigravity CLI",
  COPILOT_CLI: "GitHub Copilot CLI",
  AIDER: "Aider",
  OPENCODE: "OpenCode",
  GROK_CLI: "Grok Build",
  KIRO_CLI: "Kiro CLI",
  QWEN_CODE: "Qwen Code",
  KIMI_CODE: "Kimi Code",
  QODER_CLI: "Qoder CLI",
  AMP: "Amp",
  CRUSH: "Crush",
  GOOSE: "Goose",
  CLINE: "Cline CLI",
  DROID: "Factory Droid",
  AUGGIE: "Augment Auggie",
  MUSE_CODE: "Muse Code",
  TRAE_CLI: "TraeCode CLI",
};

/** Display name from the last CLI scan, then the whitelist; the pack's provider id otherwise. */
function useProviderName() {
  const providers = useProviders((st) => st.providers);
  return useMemo(() => {
    const names = new Map<string, string>([...Object.entries(CLI_NAMES), ...providers.map((p) => [p.id, p.name] as const)]);
    return (id: string) => names.get(id) ?? id;
  }, [providers]);
}

/** Where a model is available: each CLI's argument and the limits that differ from the model's own. */
function Offerings({ model: m, providerName }: { model: PackModel; providerName: (id: string) => string }) {
  const { t } = useTranslation();
  const limit = (o: PackOffering) =>
    [
      o.contextWindow && t("settings.knowledge.models.context", { value: fmtTokens(o.contextWindow) }),
      o.creditMultiplier !== null && t("settings.knowledge.models.credits", { value: o.creditMultiplier }),
    ]
      .filter(Boolean)
      .join(" · ");
  return (
    <table className="w-full text-left text-xs">
      <caption className="pb-1 text-left text-[11px] font-medium text-muted">{t("settings.knowledge.models.offeredBy")}</caption>
      <thead className="sr-only">
        <tr>
          <th scope="col">{t("settings.knowledge.models.cli")}</th>
          <th scope="col">{t("settings.knowledge.models.arg")}</th>
          <th scope="col">{t("settings.knowledge.models.limits")}</th>
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {m.offerings.map((o) => (
          <tr key={o.provider}>
            <th scope="row" className="py-1 pr-3 font-normal whitespace-nowrap text-fg">
              {providerName(o.provider)}
              {o.billing && <span className="ml-1.5 text-[10px] text-subtle">{t(`settings.knowledge.models.billing.${o.billing}`, o.billing)}</span>}
            </th>
            <td className="py-1 pr-3">
              <code className="font-mono text-[11px] text-muted" data-selectable>
                {o.cliModelArg}
              </code>
              {!o.verified && (
                <span className="ml-1.5 text-[10px] text-warning" title={t("settings.knowledge.models.unverifiedDesc")}>
                  {t("settings.knowledge.models.unverified")}
                </span>
              )}
            </td>
            <td className="py-1 text-right text-[11px] text-subtle">{limit(o)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** One domain weight on the pack's 1–5 scale. */
function SkillBar({ domain, score }: { domain: string; score: number }) {
  const pct = ((score - 1) / 4) * 100;
  return (
    <div className="flex items-center gap-2">
      <dt className="w-24 shrink-0 truncate text-xs text-muted" title={domain}>
        {domain}
      </dt>
      <dd className="flex flex-1 items-center gap-2">
        <span
          role="meter"
          aria-label={domain}
          aria-valuemin={1}
          aria-valuemax={5}
          aria-valuenow={score}
          className="h-1.5 flex-1 overflow-hidden rounded-full bg-active"
        >
          <span
            className={cn("block h-full rounded-full", score >= 4.5 ? "bg-accent" : score >= 3 ? "bg-accent/70" : "bg-subtle")}
            style={{ width: `${Math.max(4, pct)}%` }}
          />
        </span>
        <span className="w-6 text-right text-xs text-fg tabular-nums">{+score.toFixed(1)}</span>
      </dd>
    </div>
  );
}
