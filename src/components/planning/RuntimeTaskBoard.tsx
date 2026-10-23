import { useModelLabel } from "../../lib/useModelLabel";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Bot, LayoutGrid, Table as TableIcon, ChevronRight, Terminal, RotateCcw, Hand, Workflow, ExternalLink, Compass } from "lucide-react";
import type { ProviderId, TaskState } from "../../lib/ipc";
import type { RuntimeSnapshot } from "../../lib/runtime";
import { cn } from "../../lib/cn";
import { Badge, Button } from "../ui";
import { ProviderLogo } from "../ProviderLogo";
import { Markdown } from "../Markdown";
import { TaskStateBadge } from "./PipelineView";
import { getDomainStyle } from "./TasksTable";

/** Same task information and board affordances as the desktop pipeline, with no local scheduler. */
export function RuntimeTaskBoard({ snapshot: s, compact = false, modelControls, onRetry, onInspect, onSwitchTab, busy = false }: {
  onSwitchTab?: (tab: "plan" | "pipeline") => void;
  snapshot: RuntimeSnapshot; compact?: boolean; busy?: boolean;
  modelControls?: (id: string) => React.ReactNode; onRetry?: (id: string) => void; onInspect?: (id: string) => void;
}) {
  const { t } = useTranslation();
  const displayModel = useModelLabel();
  const [mode, setMode] = useState<"dag" | "table">("dag");
  const [selected, setSelected] = useState<string | null>(null);
  const tasks = s.plan?.plan.tasks ?? [];
  const stateOf = (id: string) => (s.execution?.tasks[id]?.state ?? "PENDING").toUpperCase() as TaskState;
  const statusBadge = (id: string) => {
    const latest = s.turns?.[s.execution?.tasks[id]?.attempts.at(-1) ?? ""];
    const interrupted = s.state === "Blocked" && stateOf(id) === "RUNNING" && latest?.taskId === id
      && (latest.status === "Unknown" || (latest.status === "Failed" && latest.ended));
    return interrupted ? <Badge size="xs" variant="warning">{t("runtime.reconciliationPending")}</Badge> : <TaskStateBadge state={stateOf(id)} />;
  };
  const done = tasks.filter(task => stateOf(task.id) === "DONE").length;
  const task = tasks.find(task => task.id === selected);
  const state = task && s.execution?.tasks[task.id];
  const attempt = state && s.work?.attempts[state.attempts.at(-1) ?? ""];
  if (!tasks.length) return null;
  const modelLabel = (id: string) => {
    const model = s.execution?.tasks[id]?.assignment ?? s.plan?.assignments[id];
    return model && <span className="inline-flex min-w-0 items-center gap-1.5 text-xs text-muted" title={displayModel(model)}><ProviderLogo id={model.provider as ProviderId} name={displayModel(model)} className="size-4 rounded [&>*]:size-3" /><span className="wrap-anywhere">{displayModel(model)}</span></span>;
  };
  return <section aria-label={t("executionBoard.ariaLabel")} className={cn("overflow-hidden rounded-xl border border-border bg-sidebar/50", !compact && "flex min-h-0 flex-1 flex-col")}>
    <header className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-base px-4 py-2.5 text-xs">
      <div className="flex min-w-0 flex-wrap items-center gap-2">{compact ? <Bot className="size-4 text-accent"/> : <Workflow className="size-4 text-accent"/>}<strong>{t(compact ? "executionBoard.title" : "pipeline.boardTitle")}</strong><Badge size="xs" variant={s.state === "Executing" ? "accent" : s.state === "Blocked" ? "warning" : "muted"}>{t(`runtime.states.${s.state}`, { defaultValue: s.state })}</Badge><span className="font-mono text-[11px] text-subtle">{t("executionBoard.completedCount", { done, total: tasks.length })}</span></div>
      <div className="flex min-w-0 flex-wrap items-center gap-1.5">
       {compact ? <>
        <button type="button" onClick={()=>onSwitchTab?.("plan")} className="inline-flex items-center gap-1 rounded border border-border bg-base px-2 py-0.5 text-[11px] font-medium text-muted transition-colors hover:bg-hover hover:text-fg"><Compass className="size-3"/>{t("workspace.tabs.plan")}</button>
        <button type="button" onClick={()=>onSwitchTab?.("pipeline")} className="inline-flex items-center gap-1 rounded border border-border bg-base px-2 py-0.5 text-[11px] font-medium text-muted transition-colors hover:bg-hover hover:text-fg"><ExternalLink className="size-3"/>{t("executionBoard.viewPipelineDag")}</button>
       </> : <>
        <div className="flex items-center rounded-md border border-border bg-base p-0.5" role="group" aria-label={t("pipeline.viewToggleAria")}>
         {(["dag","table"] as const).map(value=><button key={value} type="button" aria-pressed={mode===value} aria-label={t(value==="dag"?"runtime.cards":"runtime.table")} onClick={()=>setMode(value)} className={cn("inline-flex items-center gap-1 rounded px-2 py-1 text-xs transition-colors",mode===value?"bg-hover font-medium text-fg":"text-muted hover:text-fg")}>{value==="dag"?<LayoutGrid className="size-3.5"/>:<TableIcon className="size-3.5"/>}<span>{t(value==="dag"?"pipeline.viewCards":"pipeline.viewTable")}</span></button>)}
        </div>
        <Button size="sm" variant="ghost" onClick={()=>onSwitchTab?.("plan")}><Compass className="size-3.5"/>{t("pipeline.backToReview")}</Button>
       </>}
      </div>
    </header>
    <div className="h-0.5 bg-hover"><div className="h-full bg-accent transition-all" style={{width:`${done / tasks.length * 100}%`}} /></div>
    <div className={cn("min-h-0", !compact && "flex flex-1 overflow-hidden")}>
      <div className={cn("min-w-0 flex-1 space-y-2.5 p-3", !compact && "overflow-y-auto")}>
        {mode === "table" && !compact ? <div className="overflow-x-auto"><table className="w-full text-left text-xs"><thead className="border-b border-border text-muted"><tr>{(["runtime.task", "plan.tasks.titleLabel", "runtime.taskStatus", "runtime.model", "runtime.scope"] as const).map(label => <th key={label} className="px-3 py-2">{t(label)}</th>)}</tr></thead><tbody>{tasks.map(item => <tr key={item.id} className="cursor-pointer border-b border-border hover:bg-hover" onClick={() => setSelected(item.id)}><td className="p-3 font-mono">{item.id}</td><td className="p-3">{item.title}</td><td className="p-3">{statusBadge(item.id)}</td><td className="p-3">{modelLabel(item.id)}</td><td className="p-3 wrap-anywhere">{item.write_scope.join(", ")}</td></tr>)}</tbody></table></div> : tasks.map(item => <article key={item.id} className={cn("min-w-0 rounded-lg border bg-elevated p-3", selected === item.id ? "border-accent bg-accent/5" : "border-border")}>
          <button className="flex w-full min-w-0 items-start justify-between gap-3 text-left" onClick={() => setSelected(selected === item.id ? null : item.id)} aria-expanded={selected === item.id}>
            <div className="min-w-0 space-y-1.5"><div className="flex min-w-0 items-baseline gap-2"><strong className="shrink-0 font-mono text-xs">{item.id}</strong><span className="text-xs font-medium wrap-anywhere">{item.title}</span></div><div className="flex flex-wrap items-center gap-2"><span className={cn("rounded border px-1.5 text-[10px]", getDomainStyle(item.domain))}>{item.domain}</span>{modelLabel(item.id)}<span className="text-[10px] text-subtle">{item.size} · {"★".repeat(item.difficulty)}</span></div></div>
            <span className="flex shrink-0 items-center gap-1">{statusBadge(item.id)}<ChevronRight className={cn("size-3 text-muted", selected === item.id && "rotate-90")} /></span>
          </button>
          {item.depends_on.length > 0 && <p className="mt-2 text-[11px] text-muted">{t("plan.tasks.depends", { deps: item.depends_on.join(", ") })}</p>}
          <p className="mt-2 font-mono text-[11px] text-subtle wrap-anywhere">{item.write_scope.join(", ")}</p>
          {compact && selected === item.id && <div className="mt-3 border-t border-border pt-3 text-xs"><Markdown text={item.goal} /><ul className="mt-2 list-disc pl-5">{item.acceptance.criteria.map((criterion, i) => <li key={i}>{criterion}</li>)}</ul></div>}
          {!compact && modelControls?.(item.id)}
          {s.execution?.tasks[item.id]?.error && <p role="alert" className="mt-2 text-xs text-danger">{s.execution.tasks[item.id].error}</p>}
          {!compact && <div className="mt-2 flex flex-wrap gap-2">{s.state === "Blocked" && stateOf(item.id) === "BLOCKED" && <Button size="sm" disabled={busy} onClick={() => onRetry?.(item.id)}><RotateCcw className="size-3" />{t("runtime.retryTask")}</Button>}{s.execution && stateOf(item.id) !== "DONE" && ["Blocked", "Stopped", "AwaitingApproval", "Approved"].includes(s.state) && <Button size="sm" disabled={busy} onClick={() => onInspect?.(item.id)}><Hand className="size-3" />{t("runtime.inspectTakeover")}</Button>}</div>}
        </article>)}
      </div>
      {!compact && task && <aside aria-label={t("runtime.taskDetails")} className="w-80 max-w-[45%] shrink-0 space-y-4 overflow-y-auto border-l border-border bg-base p-4 text-xs">
        <header className="space-y-2"><strong>{task.id} · {task.title}</strong>{modelLabel(task.id)}</header>
        <Markdown text={task.goal} />
        <div><h3 className="mb-1 font-semibold">{t("runtime.scope")}</h3><p className="font-mono wrap-anywhere">{task.write_scope.join("\n")}</p></div>
        <div><h3 className="mb-1 font-semibold">{t("plan.review.tabContracts")}</h3>{task.contracts.map(id => <details key={id}><summary className="cursor-pointer py-1">{id}</summary><Markdown text={s.plan?.plan.contracts.find(c => c.id === id)?.content ?? id} /></details>)}</div>
        <div><h3 className="mb-1 font-semibold">{t("runtime.commands")}</h3>{task.acceptance.commands.map(command => <p key={command} className="flex items-start gap-1.5 py-1 font-mono wrap-anywhere"><Terminal className="mt-0.5 size-3 shrink-0" />{command}</p>)}<ul className="mt-2 list-disc space-y-1 pl-4">{task.acceptance.criteria.map((criterion, i) => <li key={i}>{criterion}</li>)}</ul></div>
        {attempt && <div><h3 className="font-semibold">{t("runtime.evidence")}</h3>{attempt.commit && <p className="mt-1 font-mono break-all">{attempt.commit}</p>}{attempt.gates.map((gate, i) => {const receipt=gate as {command?:string;result?:{passed:boolean;exitCode:number|null;outputTail:string;errorMessage:string|null}};return <details key={i} className="mt-2 rounded border border-border bg-elevated p-2"><summary className="cursor-pointer font-mono wrap-anywhere">{receipt.command ?? t("runtime.evidence")} · {receipt.result ? (receipt.result.passed ? "PASS" : "FAIL") : t("runtime.pending")}</summary><pre className="mt-2 whitespace-pre-wrap break-words text-subtle">{receipt.result?.outputTail}{receipt.result?.errorMessage}</pre></details>;})}</div>}
        {Object.values(s.turns ?? {}).filter(turn => turn.taskId === task.id).map(turn => <div key={turn.id} className="rounded border border-border p-2"><p>{turn.role} · {displayModel({ ...turn.requested, provider: turn.observedProvider ?? turn.requested.provider, requestedModel: turn.observedModel ?? turn.requested.requestedModel })}</p><p>{turn.status} · {t("runtime.cost", { cost: turn.costUsd == null ? t("runtime.unknownValue") : `$${turn.costUsd.toFixed(4)}` })}</p></div>)}
      </aside>}
    </div>
  </section>;
}
