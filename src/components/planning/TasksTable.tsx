import { useModelLabel } from "../../lib/useModelLabel";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import {
  AlertCircle,
  AlertTriangle,
  ChevronRight,
  Edit3,
  Layers,
  Plus,
  ShieldAlert,
  Terminal,
  Trash2,
  Zap,
} from "lucide-react";
import { Badge, Button } from "../ui";
import { cn } from "../../lib/cn";
import type { ModelParams, ProviderId, TaskAssignment, TaskItem, TaskSize, ValidationIssue } from "../../lib/ipc";
import { ProviderLogo } from "../ProviderLogo";
import { localizeIssue } from "../../lib/planIssue";

export function providerLabel(id: ProviderId | string): string {
  switch (id) {
    case "CLAUDE_CODE":
      return "Claude Code";
    case "CODEX_CLI":
      return "Codex CLI";
    case "CURSOR_CLI":
      return "Cursor CLI";
    case "COPILOT_CLI":
      return "GitHub Copilot";
    case "ANTIGRAVITY_CLI":
      return "Antigravity";
    case "AIDER":
      return "Aider";
    case "OPENCODE":
      return "OpenCode";
    case "REASONIX":
      return "Reasonix";
    default:
      return id;
  }
}

import { TaskModelPicker } from "./TaskModelPicker";
export { TaskModelPicker, type TaskModelChoice } from "./TaskModelPicker";

const DOMAIN_STYLES: Record<string, string> = {
  "backend-api": "border-blue-500/30 bg-blue-500/10 text-blue-400",
  "frontend-ui": "border-success/30 bg-success/10 text-success",
  database: "border-purple-500/30 bg-purple-500/10 text-purple-400",
  devops: "border-orange-500/30 bg-orange-500/10 text-orange-400",
  testing: "border-warning/30 bg-warning/10 text-warning",
  security: "border-rose-500/30 bg-rose-500/10 text-rose-400",
  docs: "border-cyan-500/30 bg-cyan-500/10 text-cyan-400",
  general: "border-zinc-500/30 bg-zinc-500/10 text-zinc-400",
};

const DOMAIN_DOTS: Record<string, string> = {
  "backend-api": "bg-[#6f93c9]",
  "frontend-ui": "bg-[#6fb08f]",
  database: "bg-[#9a8fc4]",
  "data-model": "bg-[#9a8fc4]",
  devops: "bg-[#c99a6f]",
  testing: "bg-[#c9a14a]",
  security: "bg-[#c97a86]",
  docs: "bg-[#6fa9b5]",
  algorithm: "bg-[#7fa7a0]",
  review: "bg-[#b08a7a]",
};

/** Quiet domain marker: a small muted dot instead of a coloured pill. */
export function getDomainDot(domain: string) {
  return DOMAIN_DOTS[domain] || "bg-subtle";
}

export function getDomainStyle(domain: string) {
  return DOMAIN_STYLES[domain] || "border-border bg-hover/50 text-muted";
}

function estimatePassRate(size: TaskSize, difficulty: number): number {
  const base = size === "S" ? 95 : size === "M" ? 85 : 72;
  const diffPenalty = (difficulty - 1) * 8;
  return Math.max(40, Math.min(99, base - diffPenalty));
}

function estimateCostLevel(size: TaskSize, t: TFunction): string {
  if (size === "S") return t("plan.tasks.costLevel.low");
  if (size === "M") return t("plan.tasks.costLevel.mid");
  return t("plan.tasks.costLevel.high");
}

export function TasksTable({
  tasks,
  assignments = [],
  validationIssues = [],
  onUpdateTask,
  onAddTask,
  onRemoveTask,
  onReassign,
  modelLocked = false,
  hideEstimates = false,
  lockedModels,
}: {
  tasks: TaskItem[];
  assignments?: TaskAssignment[];
  validationIssues?: ValidationIssue[];
  onUpdateTask: (id: string, patch: Partial<TaskItem>) => void;
  onAddTask: (task: TaskItem) => void;
  onRemoveTask: (id: string) => void;
  onReassign?: (taskId: string, modelSpec: string, params?: ModelParams | null) => void;
  /** Single-model mode: hide per-task reassignment. */
  modelLocked?: boolean;
  /** A durable assignment may not include estimates; do not invent them. */
  hideEstimates?: boolean;
  lockedModels?: Record<string, { provider: string; name: string }>;
}) {
  const { t } = useTranslation();
  const modelLabel = useModelLabel();

  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);

  // Edit form state
  const [editDraft, setEditDraft] = useState<TaskItem | null>(null);

  const startEdit = (task: TaskItem) => {
    setEditingId(task.id);
    setEditDraft({ ...task });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditDraft(null);
  };

  const saveEdit = () => {
    if (editDraft) {
      onUpdateTask(editDraft.id, editDraft);
    }
    setEditingId(null);
    setEditDraft(null);
  };

  const handleAddNewTask = () => {
    const nextIdx = tasks.length + 1;
    const newTask: TaskItem = {
      id: `T${nextIdx}`,
      domain: "general",
      title: t("plan.tasks.defaults.title", { index: nextIdx }),
      goal: t("plan.tasks.defaults.goal"),
      depends_on: tasks.length > 0 ? [tasks[tasks.length - 1].id] : [],
      contracts: [],
      write_scope: ["src/**"],
      deliverables: [t("plan.tasks.defaults.deliverable")],
      acceptance: {
        commands: [],
        criteria: [t("plan.tasks.defaults.criterion")],
      },
      size: "M",
      difficulty: 2,
    };
    onAddTask(newTask);
    startEdit(newTask);
  };

  return (
    <div className="flex h-full flex-col overflow-y-auto px-6 pt-6 pb-28">
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
        {/* Header bar */}
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-xl font-semibold tracking-[-0.022em] text-fg">{t("plan.tasks.title")}</h2>
            <p className="mt-1 text-[13px] text-muted">
              {t("plan.tasks.subtitle", { count: tasks.length })}
            </p>
          </div>
          <Button size="sm" variant="ghost" onClick={handleAddNewTask} className="gap-1.5 text-xs">
            <Plus className="size-3.5" />
            <span>{t("plan.tasks.add")}</span>
          </Button>
        </div>

        {/* Task list */}
        {tasks.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-border p-12 text-center">
            <Layers className="size-8 text-subtle mb-2" />
            <p className="text-xs text-muted">{t("plan.tasks.empty")}</p>
          </div>
        ) : (
          <div className="flex flex-col divide-y divide-line rounded-xl surface-card">
            {tasks.map((task) => {
              const isExpanded = expandedId === task.id;
              const isEditing = editingId === task.id;
              const issues = validationIssues.filter((i) => i.taskId === task.id);
              const hasError = issues.some((i) => i.severity === "error");
              const hasWarning = issues.some((i) => i.severity === "warning");
              const taskAssign = assignments.find((a) => a.taskId === task.id);
              const passRate = estimatePassRate(task.size, task.difficulty);
              const costLevel = estimateCostLevel(task.size, t);

              if (isEditing && editDraft && editDraft.id === task.id) {
                return (
                  <div
                    key={task.id}
                    className="m-1 flex flex-col gap-4 rounded-lg bg-base p-5 shadow-[0_0_0_1px_var(--line-strong)]"
                  >
                    <div className="flex items-center justify-between border-b border-line pb-3">
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-xs text-subtle">{editDraft.id}</span>
                        <span className="text-xs font-semibold text-fg">{t("plan.tasks.editDetails")}</span>
                      </div>
                      <div className="flex items-center gap-2">
                        <Button size="sm" variant="ghost" onClick={cancelEdit} className="text-xs">
                          {t("common.cancel")}
                        </Button>
                        <Button size="sm" variant="primary" onClick={saveEdit} className="text-xs">
                          {t("plan.tasks.save")}
                        </Button>
                      </div>
                    </div>

                    <div className="grid grid-cols-2 gap-4">
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">{t("plan.tasks.titleLabel")}</label>
                        <input
                          type="text"
                          value={editDraft.title}
                          onChange={(e) => setEditDraft({ ...editDraft, title: e.target.value })}
                          className="h-8 rounded-md border border-line-strong bg-elevated px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                        />
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">{t("plan.tasks.domainLabel")}</label>
                        <input
                          type="text"
                          value={editDraft.domain}
                          onChange={(e) => setEditDraft({ ...editDraft, domain: e.target.value })}
                          className="h-8 rounded-md border border-line-strong bg-elevated px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder="backend-api, frontend-ui, database, etc."
                        />
                      </div>
                    </div>

                    <div className="flex flex-col gap-1.5">
                      <label className="text-xs font-medium text-muted">{t("plan.tasks.goalLabel")}</label>
                      <textarea
                        rows={2}
                        value={editDraft.goal}
                        onChange={(e) => setEditDraft({ ...editDraft, goal: e.target.value })}
                        className="rounded-md border border-line-strong bg-elevated p-2 text-xs text-fg focus:border-accent focus:outline-hidden"
                      />
                    </div>

                    <div className="grid grid-cols-3 gap-4">
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">{t("plan.tasks.sizeLabel")}</label>
                        <select
                          value={editDraft.size}
                          onChange={(e) => setEditDraft({ ...editDraft, size: e.target.value as TaskSize })}
                          className="h-8 rounded-md border border-line-strong bg-elevated px-2 text-xs text-fg focus:border-accent focus:outline-hidden"
                        >
                          <option value="S">{t("plan.tasks.sizeS")}</option>
                          <option value="M">{t("plan.tasks.sizeM")}</option>
                          <option value="L">{t("plan.tasks.sizeL")}</option>
                        </select>
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">{t("plan.tasks.difficultyLabel")}</label>
                        <select
                          value={editDraft.difficulty}
                          onChange={(e) => setEditDraft({ ...editDraft, difficulty: Number(e.target.value) })}
                          className="h-8 rounded-md border border-line-strong bg-elevated px-2 text-xs text-fg focus:border-accent focus:outline-hidden"
                        >
                          <option value={1}>{t("plan.tasks.diff1")}</option>
                          <option value={2}>{t("plan.tasks.diff2")}</option>
                          <option value={3}>{t("plan.tasks.diff3")}</option>
                        </select>
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">{t("plan.tasks.modelLabel")}</label>
                        {modelLocked ? <span className="text-xs text-muted">{lockedModels?.[task.id]?.name ?? (editDraft.assigned_model ? modelLabel(editDraft.assigned_model) : t("plan.tasks.singleModelLocked"))}</span> : <TaskModelPicker
                          value={editDraft.assigned_model}
                          params={editDraft.model_params}
                          placeholder={t("plan.tasks.modelPlaceholderGlobal")}
                          onChange={(c) => setEditDraft({ ...editDraft, assigned_model: c.spec, model_params: c.params })}
                          onReset={() => setEditDraft({ ...editDraft, assigned_model: null, model_params: null })}
                        />}
                      </div>
                    </div>

                    <div className="grid grid-cols-2 gap-4">
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">{t("plan.tasks.dependsLabel")}</label>
                        <input
                          type="text"
                          value={editDraft.depends_on.join(", ")}
                          onChange={(e) =>
                            setEditDraft({
                              ...editDraft,
                              depends_on: e.target.value
                                .split(",")
                                .map((s) => s.trim())
                                .filter(Boolean),
                            })
                          }
                          className="h-8 rounded-md border border-line-strong bg-elevated px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder={t("plan.tasks.dependsPlaceholder")}
                        />
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">{t("plan.tasks.contractsLabel")}</label>
                        <input
                          type="text"
                          value={editDraft.contracts.join(", ")}
                          onChange={(e) =>
                            setEditDraft({
                              ...editDraft,
                              contracts: e.target.value
                                .split(",")
                                .map((s) => s.trim())
                                .filter(Boolean),
                            })
                          }
                          className="h-8 rounded-md border border-line-strong bg-elevated px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder={t("plan.tasks.contractsPlaceholder")}
                        />
                      </div>
                    </div>

                    <div className="grid grid-cols-2 gap-4">
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">{t("plan.tasks.writeScopeLabel")}</label>
                        <input
                          type="text"
                          value={editDraft.write_scope.join(", ")}
                          onChange={(e) =>
                            setEditDraft({
                              ...editDraft,
                              write_scope: e.target.value
                                .split(",")
                                .map((s) => s.trim())
                                .filter(Boolean),
                            })
                          }
                          className="h-8 rounded-md border border-line-strong bg-elevated px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder={t("plan.tasks.writeScopePlaceholder")}
                        />
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">{t("plan.tasks.acceptanceLabel")}</label>
                        <input
                          type="text"
                          value={editDraft.acceptance.commands.join(", ")}
                          onChange={(e) =>
                            setEditDraft({
                              ...editDraft,
                              acceptance: {
                                ...editDraft.acceptance,
                                commands: e.target.value
                                  .split(",")
                                  .map((s) => s.trim())
                                  .filter(Boolean),
                              },
                            })
                          }
                          className="h-8 rounded-md border border-line-strong bg-elevated px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder={t("plan.tasks.acceptancePlaceholder")}
                        />
                      </div>
                    </div>
                  </div>
                );
              }

              const autoName = taskAssign
                ? modelLabel({ provider: taskAssign.provider, requestedModel: taskAssign.cliModelArg, modelName: taskAssign.displayName })
                : null;
              const pct = taskAssign ? Math.round(taskAssign.pSuccess * 100) : passRate;
              const lowPass = pct < 65;
              const needsModel = task.difficulty >= 3 && !task.assigned_model && !taskAssign?.meetsThreshold;

              return (
                <div
                  key={task.id}
                  className={cn(
                    "group/row relative flex flex-col transition-colors first:rounded-t-xl last:rounded-b-xl",
                    hasError ? "bg-danger/[0.04]" : hasWarning ? "bg-warning/[0.04]" : "hover:bg-hover/40",
                  )}
                >
                  {/* Primary row: ID · title + quiet meta line · cost · model · actions */}
                  <div className="flex items-center gap-3 py-3 pl-2 pr-3">
                    <button
                      type="button"
                      onClick={() => setExpandedId(isExpanded ? null : task.id)}
                      aria-label={isExpanded ? t("plan.tasks.collapseDetails") : t("plan.tasks.expandDetails")}
                      aria-expanded={isExpanded}
                      className="inline-flex size-6 shrink-0 items-center justify-center rounded-md text-subtle transition-colors hover:bg-hover hover:text-fg"
                    >
                      <ChevronRight className={cn("size-3.5 transition-transform duration-200", isExpanded && "rotate-90")} />
                    </button>
                    <span className="w-12 shrink-0 font-mono text-xs text-subtle">{task.id}</span>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[13.5px] font-medium tracking-[-0.006em] text-fg" title={task.title}>
                        {task.title}
                      </div>
                      <div className="mt-0.5 flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5 text-xs text-subtle">
                        <span className="inline-flex items-center gap-1.5 text-muted">
                          <span className={cn("size-1.5 shrink-0 rounded-full", getDomainDot(task.domain))} />
                          {task.domain}
                        </span>
                        <span aria-hidden="true">·</span>
                        <span>{t("plan.tasks.sizeShort", { size: task.size })}</span>
                        <span aria-hidden="true">·</span>
                        <span className="inline-flex items-center gap-0.5" title={t("plan.tasks.difficultyLabel")} aria-label={`${t("plan.tasks.difficultyLabel")} ${task.difficulty}`}>
                          {[1, 2, 3].map((n) => (
                            <span key={n} className={cn("size-[5px] rounded-full", n <= task.difficulty ? "bg-muted" : "bg-subtle/35")} />
                          ))}
                        </span>
                        <span aria-hidden="true">·</span>
                        {task.depends_on.length === 0 ? (
                          <span className="inline-flex items-center gap-1 text-muted">
                            <Zap className="size-3" />
                            {t("plan.tasks.parallelStart")}
                          </span>
                        ) : (
                          <span className="truncate" title={t("plan.tasks.dependsTitle", { deps: task.depends_on.join(", ") })}>
                            {t("plan.tasks.depends", { deps: task.depends_on.join(", ") })}
                          </span>
                        )}
                        {taskAssign?.reuseAttemptId && (
                          <>
                            <span aria-hidden="true">·</span>
                            <span className="text-muted">{t("plan.tasks.windowReuse")}</span>
                          </>
                        )}
                        {taskAssign && !taskAssign.meetsThreshold && (
                          <>
                            <span aria-hidden="true">·</span>
                            <span className="text-warning">{t("plan.tasks.needsTuning")}</span>
                          </>
                        )}
                        {needsModel && (
                          <>
                            <span aria-hidden="true">·</span>
                            <span className="inline-flex items-center gap-1 text-warning" title={t("plan.tasks.difficultyWarning")}>
                              <ShieldAlert className="size-3" />
                              {t("plan.tasks.difficultyWarningShort")}
                            </span>
                          </>
                        )}
                      </div>
                    </div>

                    {!hideEstimates && (
                      <div className="flex shrink-0 flex-col items-end text-xs leading-tight tabular-nums">
                        {taskAssign ? (
                          <span className="text-[12.5px] text-muted" title={t("plan.tasks.estCostTitle", { cost: taskAssign.estCostUsd })}>
                            ${taskAssign.estCostUsd.toFixed(2)}
                          </span>
                        ) : (
                          <span className="text-[12.5px] text-muted" title={t("plan.tasks.estCostLevelTitle", { level: costLevel })}>
                            {costLevel}
                          </span>
                        )}
                        <span
                          className={cn("mt-0.5", lowPass ? "text-warning" : "text-subtle")}
                          title={taskAssign ? t("plan.tasks.passRateAutoTitle", { pct }) : t("plan.tasks.passRateEstTitle", { pct })}
                        >
                          {t("plan.tasks.passRate")} {pct}%
                        </span>
                      </div>
                    )}

                    <div className="w-52 shrink-0">
                      {modelLocked ? (
                        <span
                          className="inline-flex min-w-0 items-center gap-1.5 text-xs text-muted"
                          title={lockedModels?.[task.id]?.name ?? t("plan.tasks.singleModelTitle")}
                        >
                          {lockedModels?.[task.id] && <ProviderLogo id={lockedModels[task.id].provider as ProviderId} name={lockedModels[task.id].provider} className="size-4 rounded [&>*]:size-3"/>}
                          <span className="truncate">{lockedModels?.[task.id]?.name ?? (task.assigned_model ? modelLabel(task.assigned_model) : autoName ?? t("plan.tasks.singleModelLocked"))}</span>
                        </span>
                      ) : (
                        <TaskModelPicker
                          value={task.assigned_model}
                          params={task.model_params}
                          placeholder={autoName ? t("plan.tasks.autoAssign", { name: autoName }) : t("plan.tasks.autoRecommend")}
                          onChange={(c) => {
                            // Prefer onReassign (updates + save + realloc); avoid double saveUserEdited.
                            if (onReassign) onReassign(task.id, c.spec, c.params);
                            else onUpdateTask(task.id, { assigned_model: c.spec, model_params: c.params });
                          }}
                          onReset={() => {
                            if (onReassign) onReassign(task.id, "");
                            else onUpdateTask(task.id, { assigned_model: null, model_params: null });
                          }}
                        />
                      )}
                    </div>

                    {/* Edit & Delete: revealed on row hover / focus */}
                    <div className="flex items-center gap-0.5 opacity-0 transition-opacity duration-150 group-hover/row:opacity-100 focus-within:opacity-100">
                      <button
                        type="button"
                        onClick={() => startEdit(task)}
                        aria-label={t("plan.tasks.editTaskAria", { id: task.id })}
                        className="inline-flex size-7 items-center justify-center rounded-md text-muted transition-colors hover:bg-hover hover:text-fg"
                        title={t("plan.tasks.editTask")}
                      >
                        <Edit3 className="size-3.5" />
                      </button>
                      <button
                        type="button"
                        onClick={() => onRemoveTask(task.id)}
                        aria-label={t("plan.tasks.deleteTaskAria", { id: task.id })}
                        className="inline-flex size-7 items-center justify-center rounded-md text-muted transition-colors hover:bg-danger/10 hover:text-danger"
                        title={t("plan.tasks.deleteTask")}
                      >
                        <Trash2 className="size-3.5" />
                      </button>
                    </div>
                  </div>

                  {/* Validation issues stay visible: they need attention */}
                  {issues.length > 0 && (
                    <div className="mb-3 ml-[92px] mr-4 flex flex-col gap-1">
                      {issues.map((iss, i) => (
                        <div key={i} className="flex items-start gap-1.5 text-xs">
                          {iss.severity === "error" ? (
                            <AlertCircle className="mt-px size-3.5 shrink-0 text-danger" />
                          ) : (
                            <AlertTriangle className="mt-px size-3.5 shrink-0 text-warning" />
                          )}
                          <span className={iss.severity === "error" ? "text-danger" : "text-warning"}>
                            {t("plan.tasks.riskRule", { rule: iss.rule, message: localizeIssue(iss) })}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}

                  {/* Expanded Body Details */}
                  {isExpanded && (
                    <div className="animate-fade-in mb-3 ml-[92px] mr-4 flex flex-col gap-2.5 rounded-lg bg-hover/60 px-4 py-3 text-xs leading-relaxed">
                      {taskAssign && (
                        <div className="flex flex-col gap-1">
                          <div className="flex items-center gap-1.5">
                            <span className="font-medium text-subtle">{t("plan.tasks.routingStrategy")} </span>
                            {taskAssign.provider && (
                              <ProviderLogo
                                id={taskAssign.provider as ProviderId}
                                name={providerLabel(taskAssign.provider)}
                                className="size-4 shrink-0 rounded-xs [&>*]:size-3"
                              />
                            )}
                            <span className="text-fg">{providerLabel(taskAssign.provider)} · {autoName}</span>
                            <span className={taskAssign.meetsThreshold ? "text-subtle" : "text-warning"}>
                              — {taskAssign.meetsThreshold ? t("plan.tasks.readyRecommend") : t("plan.tasks.needsTuning")}
                            </span>
                          </div>
                          {taskAssign.explanation && <p className="text-subtle">{taskAssign.explanation}</p>}
                        </div>
                      )}
                      <div>
                        <span className="font-medium text-subtle">{t("plan.tasks.goal")} </span>
                        <span className="text-fg">{task.goal}</span>
                      </div>

                      {task.contracts.length > 0 && (
                        <div className="flex items-center gap-1.5">
                          <span className="font-medium text-subtle">{t("plan.tasks.contractRefs")} </span>
                          <div className="flex flex-wrap gap-1">
                            {task.contracts.map((cid) => (
                              <Badge key={cid} variant="muted" size="xs" className="font-mono">
                                {cid}
                              </Badge>
                            ))}
                          </div>
                        </div>
                      )}

                      {task.write_scope.length > 0 && (
                        <div className="flex items-center gap-1.5">
                          <span className="font-medium text-subtle">{t("plan.tasks.writeScope")} </span>
                          <div className="flex flex-wrap gap-1">
                            {task.write_scope.map((ws, i) => (
                              <code
                                key={i}
                                className="rounded bg-active/70 px-1.5 py-0.5 font-mono text-xs text-fg"
                              >
                                {ws}
                              </code>
                            ))}
                          </div>
                        </div>
                      )}

                      {task.deliverables.length > 0 && (
                        <div className="flex items-center gap-1.5">
                          <span className="font-medium text-subtle">{t("plan.tasks.deliverables")} </span>
                          <div className="flex flex-wrap gap-1">
                            {task.deliverables.map((d, i) => (
                              <span key={i} className="rounded bg-active/70 px-1.5 py-0.5 text-xs text-fg">
                                {d}
                              </span>
                            ))}
                          </div>
                        </div>
                      )}

                      {task.acceptance.commands.length > 0 && (
                        <div className="flex flex-col gap-1">
                          <span className="font-medium text-subtle">{t("plan.tasks.acceptanceCommands")} </span>
                          <div className="flex flex-wrap gap-1.5">
                            {task.acceptance.commands.map((cmd, i) => {
                              const taskIssues = validationIssues.filter(
                                (iss) => iss.taskId === task.id && ((iss.params ?? []).includes(cmd) || iss.message.includes(cmd)),
                              );
                              const unsafe = taskIssues.some((iss) => iss.rule === "unsafe_acceptance_command");
                              const confirmed =
                                task.acceptance.confirmedCommands?.includes(cmd) === true ||
                                taskIssues.some((iss) => iss.rule === "acceptance_command_confirmed");
                              const needsConfirm = taskIssues.some(
                                (iss) => iss.rule === "unconfirmed_acceptance_command",
                              );
                              const badgeKey = unsafe
                                ? "unsafe"
                                : confirmed
                                  ? "confirmed"
                                  : needsConfirm
                                    ? "unconfirmed"
                                    : "whitelist";
                              const badge = {
                                key: badgeKey,
                                label: t(`plan.tasks.commandBadge.${badgeKey}`),
                                className:
                                  badgeKey === "unsafe"
                                    ? "bg-danger/15 text-danger border-danger/30"
                                    : badgeKey === "confirmed"
                                      ? "bg-accent/15 text-accent border-accent/30"
                                      : badgeKey === "unconfirmed"
                                        ? "bg-warning/15 text-warning border-warning/30"
                                        : "bg-success/15 text-success border-success/30",
                              };
                              return (
                                <div
                                  key={i}
                                  className="flex items-center gap-1.5 rounded-md surface-card px-2 py-0.5 font-mono text-xs text-fg"
                                >
                                  <Terminal className="size-3 text-muted" />
                                  <code>{cmd}</code>
                                  <span
                                    className={cn(
                                      "rounded border px-1 py-px text-[10px] font-sans font-medium",
                                      badge.className,
                                    )}
                                  >
                                    {badge.label}
                                  </span>
                                  {badge.key === "unconfirmed" && (
                                    <button
                                      type="button"
                                      className="rounded border border-border bg-base px-1.5 py-px text-[10px] font-sans text-fg hover:border-accent hover:text-accent"
                                      onClick={() => {
                                        const confirmedCommands = [
                                          ...(task.acceptance.confirmedCommands ?? []),
                                          cmd,
                                        ].filter((c, idx, arr) => arr.indexOf(c) === idx);
                                        onUpdateTask(task.id, {
                                          acceptance: {
                                            ...task.acceptance,
                                            confirmedCommands,
                                          },
                                        });
                                      }}
                                    >
                                      {t("plan.tasks.confirmCommand")}
                                    </button>
                                  )}
                                </div>
                              );
                            })}
                          </div>
                        </div>
                      )}

                      {task.acceptance.criteria.length > 0 && (
                        <div className="flex flex-col gap-1">
                          <span className="font-medium text-subtle">{t("plan.tasks.criteria")} </span>
                          <ul className="list-disc pl-4 space-y-0.5 text-fg">
                            {task.acceptance.criteria.map((crit, i) => (
                              <li key={i}>{crit}</li>
                            ))}
                          </ul>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
