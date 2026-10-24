import { useMemo, useState } from "react";
import { parseDocument } from "yaml";
import { useTranslation } from "react-i18next";

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

/** Presentation only: never replaces the runtime's authoritative plan parser. */
function readTasks(source: string): Record<string, unknown>[] {
  try {
    const doc = parseDocument(source, { uniqueKeys: true });
    if (doc.errors.length) return [];
    const value: unknown = doc.toJS({ maxAliasCount: 50 });
    const tasks = record(value) ? value.tasks : value;
    return Array.isArray(tasks) ? tasks.filter(record) : [];
  } catch { return []; }
}

function Value({ value }: { value: unknown }) {
  if (Array.isArray(value)) return value.length ? <ul className="space-y-1">{value.map((item, i) => <li key={i}><Value value={item}/></li>)}</ul> : <span className="text-subtle">—</span>;
  if (record(value)) return <dl className="space-y-2">{Object.entries(value).map(([key, item]) => <div key={key}><dt className="text-xs text-subtle">{key.replaceAll("_", " ")}</dt><dd><Value value={item}/></dd></div>)}</dl>;
  return <span className="whitespace-pre-wrap break-words">{value == null ? "—" : String(value)}</span>;
}

export function TaskCards({ source }: { source: string }) {
  const { t } = useTranslation();
  const [raw, setRaw] = useState(false);
  const tasks = useMemo(() => readTasks(source), [source]);
  return <div className="my-2 space-y-3">
    <div role="group" aria-label={t("markdown.taskView")} className="flex justify-end gap-1 text-xs">
      <button type="button" aria-pressed={!raw} onClick={() => setRaw(false)} className="rounded border border-border px-2 py-1 aria-pressed:bg-hover">{t("markdown.cards")}</button>
      <button type="button" aria-pressed={raw} onClick={() => setRaw(true)} className="rounded border border-border px-2 py-1 aria-pressed:bg-hover">{t("markdown.source")}</button>
    </div>
    {raw ? <pre className="max-h-[550px] overflow-auto whitespace-pre-wrap rounded-lg border border-border bg-sidebar p-3 text-xs"><code>{source}</code></pre> : tasks.length ? tasks.map((task, index) => <article key={index} className="rounded-lg border border-border bg-sidebar p-3">
      <div className="mb-3 flex items-baseline gap-2"><span className="font-mono text-xs text-accent">{String(task.id ?? index + 1)}</span><h4 className="font-semibold">{typeof task.title === "string" ? task.title : t("markdown.task")}</h4></div>
      <dl className="space-y-3 text-sm">{Object.entries(task).filter(([key]) => key !== "id" && key !== "title").map(([key, value]) => <div key={key}><dt className="mb-1 text-xs text-subtle">{t(`markdown.taskFields.${key}`, { defaultValue: key.replaceAll("_", " ") })}</dt><dd><Value value={value}/></dd></div>)}</dl>
    </article>) : <p className="text-sm text-subtle">{t("markdown.tasksPending")}</p>}
  </div>;
}
