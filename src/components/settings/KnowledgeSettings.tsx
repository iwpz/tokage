import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, ChevronDown, Download, Loader2, RotateCcw, ShieldAlert } from "lucide-react";
import { cn } from "../../lib/cn";
import { parsePack, type PackModel } from "../../lib/knowledge";
import builtinPackText from "../../../src-tauri/resources/knowledge/builtin.json?raw";
import { useKnowledge } from "../../stores/knowledge";
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
      ? models.filter((m) => [m.displayName, m.modelId, m.provider, ...m.tags].some((x) => x.toLowerCase().includes(q)))
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
            <ModelRow key={`${m.provider}:${m.modelId}`} model={m} domains={domains} />
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
  const id = `pack-model-${m.provider}-${m.modelId}`.replace(/[^\w-]/g, "_");

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
            <span className="rounded bg-hover px-1.5 text-[10px] text-muted">{m.provider}</span>
            {m.roles.map((r) => (
              <span key={r} className="rounded bg-accent-soft px-1.5 text-[10px] text-accent">
                {r}
              </span>
            ))}
          </div>
          {!open && top.length > 0 && (
            <div className="mt-1 truncate text-xs text-muted">
              {top.map((d) => `${d} ${m.skills[d]}`).join(" · ")}
            </div>
          )}
        </div>
        <ChevronDown aria-hidden className={cn("size-4 shrink-0 text-subtle transition-transform", open && "rotate-180")} />
      </button>

      {open && (
        <div id={id} className="space-y-3 px-4 pb-4">
          {m.description && <p className="text-xs leading-relaxed text-muted">{m.description}</p>}
          {facts.length > 0 && <p className="text-[11px] text-subtle">{facts.join(" · ")}</p>}
          {keys.length > 0 ? (
            <dl className="grid grid-cols-1 gap-x-6 gap-y-1.5 sm:grid-cols-2">
              {keys.map((d) => (
                <SkillBar key={d} domain={d} score={m.skills[d]} />
              ))}
            </dl>
          ) : (
            <p className="text-xs text-subtle">{t("settings.knowledge.models.noSkills")}</p>
          )}
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
