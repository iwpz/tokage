import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Compass, Code2, ListTodo, Save } from "lucide-react";
import type { RuntimeModel } from "../../lib/runtime";
import type { Plan } from "../../lib/ipc";
import { cn } from "../../lib/cn";
import { Badge, Button } from "../ui";
import { SpecEditor } from "./SpecEditor";
import { ContractsEditor } from "./ContractsEditor";
import { TasksTable } from "./TasksTable";

/** Existing structured plan editors; saves are explicit runtime intents, never legacy persistence. */
export function RuntimePlanReview({ plan, editable, dirty, busy, onChange, onSave, taskModels, assignments }: {
  assignments?: Record<string, RuntimeModel>;
  plan: Plan; editable: boolean; dirty: boolean; busy: boolean;
  onChange: (plan: Plan) => void; onSave: () => void; taskModels?: React.ReactNode;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<"spec" | "contracts" | "tasks">("spec");
  const tabs = [{ id: "spec", icon: Compass, label: "plan.review.tabSpec", count: null }, { id: "contracts", icon: Code2, label: "plan.review.tabContracts", count: plan.contracts.length }, { id: "tasks", icon: ListTodo, label: "plan.review.tabTasks", count: plan.tasks.length }] as const;
  return <section className="flex min-h-0 flex-1 flex-col overflow-hidden bg-base" aria-label={t("workspace.tabs.plan")}>
    <header className="flex flex-wrap items-center justify-between gap-2 border-b border-border px-5 py-2.5">
      <div role="tablist" aria-label={t("workspace.tabs.plan")} className="flex gap-1">{tabs.map(({ id, icon: Icon, label, count }) => <button key={id} role="tab" aria-selected={tab === id} onClick={() => setTab(id)} className={cn("inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium", tab === id ? "bg-active text-fg" : "text-muted hover:bg-hover")}><Icon className="size-3.5" />{t(label)}{count != null && <Badge size="xs">{count}</Badge>}</button>)}</div>
      <div className="flex items-center gap-2">{dirty && <Badge variant="warning" size="xs">{t("plan.review.unsaved")}</Badge>}<Button size="sm" variant="primary" disabled={!editable || !dirty || busy} onClick={onSave}><Save className="size-3.5" />{t("plan.review.save")}</Button></div>
    </header>
    <fieldset disabled={!editable || busy} className="m-0 min-h-0 flex-1 overflow-hidden border-0 p-0">
      {tab === "spec" && <SpecEditor spec={plan.spec} onChange={patch => onChange({ ...plan, spec: { ...plan.spec, ...patch } })} />}
      {tab === "contracts" && <ContractsEditor contracts={plan.contracts} onUpdate={(id, patch) => onChange({ ...plan, contracts: plan.contracts.map(c => c.id === id ? { ...c, ...patch } : c) })} onAdd={c => onChange({ ...plan, contracts: [...plan.contracts, c] })} onRemove={id => onChange({ ...plan, contracts: plan.contracts.filter(c => c.id !== id) })} />}
      {tab === "tasks" && <div className="h-full overflow-y-auto"><TasksTable tasks={plan.tasks} modelLocked hideEstimates lockedModels={Object.fromEntries(Object.entries(assignments??{}).map(([id, model])=>[id,{provider:model.provider,name:model.requestedModel}]))} onUpdateTask={(id, patch) => onChange({ ...plan, tasks: plan.tasks.map(task => task.id === id ? { ...task, ...patch } : task) })} onAddTask={task => onChange({ ...plan, tasks: [...plan.tasks, task] })} onRemoveTask={id => onChange({ ...plan, tasks: plan.tasks.filter(task => task.id !== id) })} />{taskModels}</div>}
    </fieldset>
  </section>;
}
