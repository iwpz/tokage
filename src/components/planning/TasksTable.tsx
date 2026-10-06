import { useState, useMemo, useRef, useEffect } from "react";
import {
  AlertCircle,
  AlertTriangle,
  Bot,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Edit3,
  Layers,
  Plus,
  ShieldAlert,
  Sparkles,
  Terminal,
  Trash2,
} from "lucide-react";
import { Badge, Button } from "../ui";
import { cn } from "../../lib/cn";
import type { ProviderId, ProviderInfo, TaskAssignment, TaskItem, TaskSize, ValidationIssue } from "../../lib/ipc";
import { useProviders } from "../../stores/providers";
import { useSettings } from "../../stores/settings";
import { isModelBlocked } from "../../lib/models";
import { ProviderLogo } from "../ProviderLogo";

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
    default:
      return id;
  }
}

interface TreeModelGroup {
  provider: ProviderInfo;
  models: { id: string; name: string }[];
}

/** Two-level hierarchical tree picker for assigning models to a task (Provider -> Model) */
function TaskModelPicker({
  value,
  placeholder,
  treeGroups,
  onChange,
  onReset,
}: {
  value?: string | null;
  placeholder: string;
  treeGroups: TreeModelGroup[];
  onChange: (val: string) => void;
  onReset: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const ref = useRef<HTMLDivElement>(null);

  // Close when clicking outside
  useEffect(() => {
    if (!open) return;
    const handleDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", handleDown);
    return () => window.removeEventListener("mousedown", handleDown);
  }, [open]);

  // Expand parent provider of current value by default
  useEffect(() => {
    if (open && value) {
      const provId = value.split(":")[0];
      if (provId) setExpanded((s) => new Set([...s, provId]));
    }
  }, [open, value]);

  const toggleGroup = (provId: string) => {
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(provId)) next.delete(provId);
      else next.add(provId);
      return next;
    });
  };

  // Find active model & provider info
  const activeSelection = useMemo(() => {
    if (!value) return null;
    for (const g of treeGroups) {
      for (const m of g.models) {
        if (`${g.provider.id}:${m.id}` === value || m.id === value) {
          return {
            providerId: g.provider.id,
            providerName: providerLabel(g.provider.id),
            modelName: m.name,
            fullLabel: `${providerLabel(g.provider.id)} · ${m.name}`,
          };
        }
      }
    }
    return {
      providerId: null,
      providerName: "",
      modelName: value,
      fullLabel: value,
    };
  }, [value, treeGroups]);

  return (
    <div ref={ref} className="relative inline-block text-left">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex h-6 max-w-[190px] items-center justify-between gap-1.5 rounded border border-border bg-base px-2 text-[11px] text-fg hover:border-accent hover:bg-hover focus:outline-hidden"
        title={activeSelection ? activeSelection.fullLabel : placeholder}
      >
        <div className="flex items-center gap-1 min-w-0">
          {activeSelection?.providerId ? (
            <ProviderLogo
              id={activeSelection.providerId}
              name={activeSelection.providerName}
              className="size-3.5 rounded-xs [&>*]:size-2.5 shrink-0"
            />
          ) : null}
          <span className="truncate">{activeSelection ? activeSelection.modelName : placeholder}</span>
        </div>
        <ChevronDown className={cn("size-3 shrink-0 text-muted transition-transform", open && "rotate-180")} />
      </button>

      {open && (
        <div className="absolute right-0 top-full z-40 mt-1 max-h-72 w-64 overflow-y-auto rounded-lg border border-border bg-elevated p-1 shadow-xl text-xs">
          <div className="flex items-center justify-between px-2 py-1 border-b border-border text-[11px] text-muted font-medium">
            <span>选择执行模型 (二级树)</span>
            {value && (
              <button
                type="button"
                onClick={() => {
                  onReset();
                  setOpen(false);
                }}
                className="text-[10px] text-accent hover:underline"
              >
                清除指定
              </button>
            )}
          </div>

          <div className="pt-1">
            {/* Auto / Default Option */}
            <button
              type="button"
              onClick={() => {
                onReset();
                setOpen(false);
              }}
              className={cn(
                "flex w-full items-center justify-between rounded px-2 py-1.5 text-left hover:bg-hover",
                !value && "bg-accent/10 font-semibold text-accent"
              )}
            >
              <div className="flex items-center gap-1.5">
                <Sparkles className="size-3 text-accent" />
                <span>自动 / 默认推荐</span>
              </div>
              {!value && <Check className="size-3 text-accent" />}
            </button>

            {treeGroups.length === 0 ? (
              <div className="px-3 py-3 text-center text-[11px] text-subtle">无可用模型</div>
            ) : (
              treeGroups.map((g) => {
                const isGroupExpanded = expanded.has(g.provider.id);
                const hasSelectedModel = g.models.some((m) => `${g.provider.id}:${m.id}` === value);

                return (
                  <div key={g.provider.id} className="mt-0.5">
                    <button
                      type="button"
                      onClick={() => toggleGroup(g.provider.id)}
                      className={cn(
                        "flex w-full items-center justify-between rounded px-2 py-1 text-left font-medium text-fg hover:bg-hover",
                        hasSelectedModel && "text-accent"
                      )}
                    >
                      <div className="flex items-center gap-1.5 min-w-0">
                        <ChevronRight
                          className={cn("size-3 text-muted shrink-0 transition-transform", isGroupExpanded && "rotate-90")}
                        />
                        <ProviderLogo
                          id={g.provider.id}
                          name={providerLabel(g.provider.id)}
                          className="size-3.5 rounded-xs [&>*]:size-2.5 shrink-0"
                        />
                        <span className="truncate">{providerLabel(g.provider.id)}</span>
                      </div>
                      <span className="text-[10px] text-subtle shrink-0">({g.models.length})</span>
                    </button>

                    {isGroupExpanded && (
                      <div className="pl-4 pr-1 py-0.5 space-y-0.5">
                        {g.models.map((m) => {
                          const modelSpec = `${g.provider.id}:${m.id}`;
                          const isSelected = value === modelSpec;

                          return (
                            <button
                              key={m.id}
                              type="button"
                              onClick={() => {
                                onChange(modelSpec);
                                setOpen(false);
                              }}
                              className={cn(
                                "flex w-full items-center justify-between rounded px-2 py-1 text-left text-[11px] hover:bg-hover",
                                isSelected ? "bg-accent/15 font-semibold text-accent" : "text-fg"
                              )}
                            >
                              <span className="truncate">{m.name}</span>
                              {isSelected && <Check className="size-3 text-accent shrink-0 ml-1" />}
                            </button>
                          );
                        })}
                      </div>
                    )}
                  </div>
                );
              })
            )}
          </div>
        </div>
      )}
    </div>
  );
}

const DOMAIN_STYLES: Record<string, string> = {
  "backend-api": "border-blue-500/30 bg-blue-500/10 text-blue-400",
  "frontend-ui": "border-emerald-500/30 bg-emerald-500/10 text-emerald-400",
  database: "border-purple-500/30 bg-purple-500/10 text-purple-400",
  devops: "border-orange-500/30 bg-orange-500/10 text-orange-400",
  testing: "border-amber-500/30 bg-amber-500/10 text-amber-400",
  security: "border-rose-500/30 bg-rose-500/10 text-rose-400",
  docs: "border-cyan-500/30 bg-cyan-500/10 text-cyan-400",
  general: "border-zinc-500/30 bg-zinc-500/10 text-zinc-400",
};

export function getDomainStyle(domain: string) {
  return DOMAIN_STYLES[domain] || "border-border bg-hover/50 text-muted";
}

function estimatePassRate(size: TaskSize, difficulty: number): number {
  const base = size === "S" ? 95 : size === "M" ? 85 : 72;
  const diffPenalty = (difficulty - 1) * 8;
  return Math.max(40, Math.min(99, base - diffPenalty));
}

function estimateCostLevel(size: TaskSize): string {
  if (size === "S") return "低 (~0.5x)";
  if (size === "M") return "中 (~1.5x)";
  return "高 (~3.0x)";
}

export function TasksTable({
  tasks,
  assignments = [],
  validationIssues = [],
  onUpdateTask,
  onAddTask,
  onRemoveTask,
  onReassign,
}: {
  tasks: TaskItem[];
  assignments?: TaskAssignment[];
  validationIssues?: ValidationIssue[];
  onUpdateTask: (id: string, patch: Partial<TaskItem>) => void;
  onAddTask: (task: TaskItem) => void;
  onRemoveTask: (id: string) => void;
  onReassign?: (taskId: string, modelSpec: string) => void;
}) {
  const providers = useProviders((s) => s.providers);
  const blockedModels = useSettings((s) => s.blockedModels);
  const banningLevel = useSettings((s) => s.modelBanningLevel);
  const modelProviderPreferences = useSettings((s) => s.modelProviderPreferences);

  // Filter models by blockedModels list, banning level, and provider preferences
  // Deduplicates combinations (e.g. Cursor variants) by using modelGroups when available
  const treeGroups = useMemo<TreeModelGroup[]>(() => {
    const groups: TreeModelGroup[] = [];
    for (const p of providers) {
      if (p.modelGroups && p.modelGroups.length > 0) {
        const filtered = p.modelGroups.filter(
          (g) =>
            !isModelBlocked(g.key, blockedModels, p.id, banningLevel, modelProviderPreferences) &&
            !isModelBlocked(g.key, blockedModels, p.name, banningLevel, modelProviderPreferences) &&
            !isModelBlocked(g.name, blockedModels, p.id, banningLevel, modelProviderPreferences) &&
            !isModelBlocked(g.name, blockedModels, p.name, banningLevel, modelProviderPreferences),
        );
        if (filtered.length > 0) {
          groups.push({
            provider: p,
            models: filtered.map((g) => ({
              id: g.variants[0]?.id || g.key,
              name: g.name || g.key,
            })),
          });
        }
      } else {
        const filtered = p.models.filter(
          (m) =>
            !isModelBlocked(m.id, blockedModels, p.id, banningLevel, modelProviderPreferences) &&
            !isModelBlocked(m.id, blockedModels, p.name, banningLevel, modelProviderPreferences) &&
            !isModelBlocked(m.name, blockedModels, p.id, banningLevel, modelProviderPreferences) &&
            !isModelBlocked(m.name, blockedModels, p.name, banningLevel, modelProviderPreferences),
        );
        if (filtered.length > 0) {
          groups.push({
            provider: p,
            models: filtered.map((m) => ({ id: m.id, name: m.name })),
          });
        }
      }
    }
    return groups;
  }, [providers, blockedModels, banningLevel, modelProviderPreferences]);

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
      title: `新任务 ${nextIdx}`,
      goal: "描述任务要达成的具体目标",
      depends_on: tasks.length > 0 ? [tasks[tasks.length - 1].id] : [],
      contracts: [],
      write_scope: ["src/**"],
      deliverables: ["功能实现与自测"],
      acceptance: {
        commands: [],
        criteria: ["单元测试通过"],
      },
      size: "M",
      difficulty: 2,
    };
    onAddTask(newTask);
    startEdit(newTask);
  };

  return (
    <div className="flex h-full flex-col overflow-y-auto p-6">
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
        {/* Header bar */}
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-sm font-semibold text-fg">任务图表与模型分配 (Tasks & Model Assignment)</h2>
            <p className="mt-0.5 text-xs text-muted">
              共 {tasks.length} 项任务。在此阶段支持手动为单个任务指定执行模型，并在执行前核查潜在冲突风险。
            </p>
          </div>
          <Button size="sm" variant="secondary" onClick={handleAddNewTask} className="gap-1.5 text-xs">
            <Plus className="size-3.5" />
            <span>添加任务</span>
          </Button>
        </div>

        {/* Task list */}
        {tasks.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-border p-12 text-center">
            <Layers className="size-8 text-subtle mb-2" />
            <p className="text-xs text-muted">暂无任务项，请在上方点击「添加任务」或由规划助手生成。</p>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            {tasks.map((task) => {
              const isExpanded = expandedId === task.id;
              const isEditing = editingId === task.id;
              const issues = validationIssues.filter((i) => i.taskId === task.id);
              const hasError = issues.some((i) => i.severity === "error");
              const hasWarning = issues.some((i) => i.severity === "warning");
              const taskAssign = assignments.find((a) => a.taskId === task.id);
              const passRate = estimatePassRate(task.size, task.difficulty);
              const costLevel = estimateCostLevel(task.size);

              if (isEditing && editDraft && editDraft.id === task.id) {
                return (
                  <div
                    key={task.id}
                    className="flex flex-col gap-4 rounded-xl border border-accent/40 bg-elevated/80 p-5 shadow-xs"
                  >
                    <div className="flex items-center justify-between border-b border-border pb-3">
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-xs font-bold text-accent">{editDraft.id}</span>
                        <span className="text-xs font-semibold text-fg">编辑任务详情</span>
                      </div>
                      <div className="flex items-center gap-2">
                        <Button size="sm" variant="ghost" onClick={cancelEdit} className="text-xs">
                          取消
                        </Button>
                        <Button size="sm" variant="primary" onClick={saveEdit} className="text-xs">
                          保存
                        </Button>
                      </div>
                    </div>

                    <div className="grid grid-cols-2 gap-4">
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">标题</label>
                        <input
                          type="text"
                          value={editDraft.title}
                          onChange={(e) => setEditDraft({ ...editDraft, title: e.target.value })}
                          className="h-8 rounded-md border border-border bg-base px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                        />
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">所属领域 (Domain)</label>
                        <input
                          type="text"
                          value={editDraft.domain}
                          onChange={(e) => setEditDraft({ ...editDraft, domain: e.target.value })}
                          className="h-8 rounded-md border border-border bg-base px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder="backend-api, frontend-ui, database, etc."
                        />
                      </div>
                    </div>

                    <div className="flex flex-col gap-1.5">
                      <label className="text-xs font-medium text-muted">目标与完成准则 (Goal)</label>
                      <textarea
                        rows={2}
                        value={editDraft.goal}
                        onChange={(e) => setEditDraft({ ...editDraft, goal: e.target.value })}
                        className="rounded-md border border-border bg-base p-2 text-xs text-fg focus:border-accent focus:outline-hidden"
                      />
                    </div>

                    <div className="grid grid-cols-3 gap-4">
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">规模 (Size)</label>
                        <select
                          value={editDraft.size}
                          onChange={(e) => setEditDraft({ ...editDraft, size: e.target.value as TaskSize })}
                          className="h-8 rounded-md border border-border bg-base px-2 text-xs text-fg focus:border-accent focus:outline-hidden"
                        >
                          <option value="S">S (小规模, 适合低耗模型)</option>
                          <option value="M">M (中等规模, 常见任务)</option>
                          <option value="L">L (大规模, 复杂度高)</option>
                        </select>
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">难度 (Difficulty 1-3)</label>
                        <select
                          value={editDraft.difficulty}
                          onChange={(e) => setEditDraft({ ...editDraft, difficulty: Number(e.target.value) })}
                          className="h-8 rounded-md border border-border bg-base px-2 text-xs text-fg focus:border-accent focus:outline-hidden"
                        >
                          <option value={1}>1 (简单清晰，单步实现)</option>
                          <option value={2}>2 (常规工程，需上下文)</option>
                          <option value={3}>3 (高难度，跨模块或重构)</option>
                        </select>
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">手动指定执行模型 (P5 二级树)</label>
                        <TaskModelPicker
                          value={editDraft.assigned_model}
                          placeholder="跟随全局 / 自动匹配"
                          treeGroups={treeGroups}
                          onChange={(val) => setEditDraft({ ...editDraft, assigned_model: val })}
                          onReset={() => setEditDraft({ ...editDraft, assigned_model: null })}
                        />
                      </div>
                    </div>

                    <div className="grid grid-cols-2 gap-4">
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">依赖上游任务 (用逗号分隔)</label>
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
                          className="h-8 rounded-md border border-border bg-base px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder="例如: T1, T2"
                        />
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">引用契约 (用逗号分隔)</label>
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
                          className="h-8 rounded-md border border-border bg-base px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder="例如: C1, C2"
                        />
                      </div>
                    </div>

                    <div className="grid grid-cols-2 gap-4">
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">允许写入路径 (Glob 格式，逗号分隔)</label>
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
                          className="h-8 rounded-md border border-border bg-base px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder="例如: src/server/**, web/src/pages/**"
                        />
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <label className="text-xs font-medium text-muted">本地验收命令 (逗号分隔)</label>
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
                          className="h-8 rounded-md border border-border bg-base px-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
                          placeholder="例如: cargo test -p server, npm test"
                        />
                      </div>
                    </div>
                  </div>
                );
              }

              return (
                <div
                  key={task.id}
                  className={cn(
                    "flex flex-col rounded-xl border bg-elevated/40 transition-colors",
                    hasError
                      ? "border-danger/40 bg-danger/5"
                      : hasWarning
                        ? "border-warning/40 bg-warning/5"
                        : "border-border hover:border-border-hover",
                  )}
                >
                  {/* Card Row Header - Structured flex-wrap / grid layout */}
                  <div className="flex flex-col gap-2.5 p-3.5 sm:flex-row sm:items-center sm:justify-between">
                    <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                      <span className="flex h-7 min-w-7 shrink-0 items-center justify-center rounded-md bg-hover px-1.5 font-mono text-xs font-bold text-fg">
                        {task.id}
                      </span>
                      <span className={cn("shrink-0 rounded-md border px-2 py-0.5 text-xs font-medium", getDomainStyle(task.domain))}>
                        {task.domain}
                      </span>
                      <span className="truncate text-xs font-semibold text-fg" title={task.title}>
                        {task.title}
                      </span>

                      {/* Dependency Badge */}
                      {task.depends_on.length === 0 ? (
                        <span className="shrink-0 rounded bg-emerald-500/10 px-1.5 py-0.5 text-[10px] text-emerald-400">
                          ⚡ 并行起点
                        </span>
                      ) : (
                        <span className="shrink-0 rounded bg-hover px-1.5 py-0.5 text-[10px] text-muted font-mono" title={`依赖: ${task.depends_on.join(", ")}`}>
                          依赖: {task.depends_on.join(", ")}
                        </span>
                      )}

                      {/* Size & Difficulty */}
                      <span className="shrink-0 rounded bg-hover px-1.5 py-0.5 text-[10px] font-mono text-muted">
                        Size: {task.size}
                      </span>
                      <span className="shrink-0 rounded bg-hover px-1.5 py-0.5 text-[10px] font-mono text-muted">
                        Diff: {"★".repeat(task.difficulty)}
                      </span>
                    </div>

                    <div className="flex shrink-0 items-center justify-between gap-3 sm:justify-end">
                      {/* Pass Rate & Cost Estimate Badges */}
                      {taskAssign ? (
                        <div className="flex items-center gap-1.5 text-[11px] text-muted">
                          <span
                            className="tabular-nums"
                            title={`自动分配一次通过把握: ${(taskAssign.pSuccess * 100).toFixed(0)}%`}
                          >
                            把握:{" "}
                            <span
                              className={cn(
                                "font-semibold",
                                taskAssign.pSuccess >= 0.8
                                  ? "text-emerald-400"
                                  : taskAssign.pSuccess >= 0.65
                                    ? "text-amber-400"
                                    : "text-rose-400"
                              )}
                            >
                              {(taskAssign.pSuccess * 100).toFixed(0)}%
                            </span>
                          </span>
                          <span>·</span>
                          <span title={`预计消耗: $${taskAssign.estCostUsd}`}>
                            ${taskAssign.estCostUsd.toFixed(2)}
                          </span>
                        </div>
                      ) : (
                        <div className="flex items-center gap-1.5 text-[11px] text-muted">
                          <span className="tabular-nums" title={`预估一次通过把握: ${passRate}%`}>
                            把握:{" "}
                            <span
                              className={cn(
                                "font-semibold",
                                passRate >= 80 ? "text-emerald-400" : passRate >= 65 ? "text-amber-400" : "text-rose-400"
                              )}
                            >
                              {passRate}%
                            </span>
                          </span>
                          <span>·</span>
                          <span title={`预估消耗成本: ${costLevel}`}>{costLevel}</span>
                        </div>
                      )}

                      {/* Assigned Model Selector with Auto-Assignment Indication (Two-Level Tree) */}
                      <div className="flex items-center gap-1">
                        <Bot className="size-3 text-muted shrink-0" />
                        <TaskModelPicker
                          value={task.assigned_model}
                          placeholder={taskAssign ? `自动: ${taskAssign.displayName}` : "自动推荐"}
                          treeGroups={treeGroups}
                          onChange={(val) => {
                            onUpdateTask(task.id, {
                              assigned_model: val,
                            });
                            if (onReassign) {
                              onReassign(task.id, val);
                            }
                          }}
                          onReset={() => {
                            onUpdateTask(task.id, {
                              assigned_model: null,
                            });
                            if (onReassign) {
                              onReassign(task.id, "");
                            }
                          }}
                        />
                      </div>

                      {/* Edit & Delete buttons */}
                      <div className="flex items-center gap-0.5">
                        <button
                          type="button"
                          onClick={() => startEdit(task)}
                          aria-label={`编辑任务 ${task.id}`}
                          className="rounded p-1 text-muted hover:bg-hover hover:text-fg"
                          title="编辑任务"
                        >
                          <Edit3 className="size-3.5" />
                        </button>
                        <button
                          type="button"
                          onClick={() => onRemoveTask(task.id)}
                          aria-label={`删除任务 ${task.id}`}
                          className="rounded p-1 text-muted hover:bg-danger/10 hover:text-danger"
                          title="删除任务"
                        >
                          <Trash2 className="size-3.5" />
                        </button>
                        <button
                          type="button"
                          onClick={() => setExpandedId(isExpanded ? null : task.id)}
                          aria-label={isExpanded ? "收起详情" : "展开详情"}
                          className="rounded p-1 text-muted hover:bg-hover hover:text-fg"
                        >
                          {isExpanded ? <ChevronUp className="size-3.5" /> : <ChevronDown className="size-3.5" />}
                        </button>
                      </div>
                    </div>
                  </div>

                  {/* Risk & Issue Banner inside task card if any */}
                  {issues.length > 0 && (
                    <div className="flex flex-col gap-1 border-t border-border/50 bg-hover/20 px-4 py-2.5">
                      {issues.map((iss, i) => (
                        <div key={i} className="flex items-center gap-1.5 text-xs">
                          {iss.severity === "error" ? (
                            <AlertCircle className="size-3.5 text-danger shrink-0" />
                          ) : (
                            <AlertTriangle className="size-3.5 text-warning shrink-0" />
                          )}
                          <span
                            className={cn(
                              "font-medium",
                              iss.severity === "error" ? "text-danger" : "text-warning",
                            )}
                          >
                            [风险: {iss.rule}] {iss.message}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}

                  {/* Auto-Assignment Explanation Banner (docs/04 §7, P6.m5d) - Collapsible */}
                  {taskAssign && (
                    <details className="group border-t border-border/40 text-xs">
                      <summary
                        className={cn(
                          "flex cursor-pointer list-none items-center justify-between px-4 py-2 select-none hover:bg-hover/30 transition-colors",
                          taskAssign.meetsThreshold
                            ? "bg-emerald-500/5 text-emerald-300"
                            : "bg-warning/10 text-warning"
                        )}
                      >
                        <div className="flex items-center gap-2 min-w-0">
                          <Bot className="size-3.5 shrink-0" />
                          <div className="flex items-center gap-1.5 min-w-0 font-medium">
                            <span className="shrink-0 text-muted">模型调度策略:</span>
                            {taskAssign.provider && (
                              <ProviderLogo
                                id={taskAssign.provider as ProviderId}
                                name={providerLabel(taskAssign.provider)}
                                className="size-4 rounded-xs [&>*]:size-3 shrink-0"
                              />
                            )}
                            <span className="truncate">
                              {providerLabel(taskAssign.provider)} · {taskAssign.displayName}
                            </span>
                            <span className="shrink-0 text-[11px] opacity-80">
                              （{taskAssign.meetsThreshold ? "推荐就绪" : "待调优"}）
                            </span>
                          </div>
                        </div>
                        <div className="flex items-center gap-2 shrink-0">
                          {taskAssign.reuseAttemptId && (
                            <span className="rounded bg-accent/10 px-1.5 py-0.5 text-[10px] text-accent font-mono">
                              ⚡ 窗口复用加成
                            </span>
                          )}
                          <span className="text-[11px] text-subtle group-open:hidden">展开详情 ▾</span>
                          <span className="text-[11px] text-subtle hidden group-open:inline">收起 ▴</span>
                        </div>
                      </summary>
                      <div className="border-t border-border/20 bg-base/40 px-4 py-2.5 text-[11px] leading-relaxed text-subtle">
                        <p>{taskAssign.explanation}</p>
                      </div>
                    </details>
                  )}

                  {/* High difficulty warning if no model is assigned and below threshold */}
                  {task.difficulty >= 3 && !task.assigned_model && !taskAssign?.meetsThreshold && (
                    <div className="flex items-center gap-1.5 border-t border-warning/20 bg-warning/5 px-4 py-1.5 text-[11px] text-warning">
                      <ShieldAlert className="size-3 shrink-0" />
                      <span>把握门槛预警: 难度 3 的任务建议手动指定高推理能力模型（如 Claude Sonnet 5.5 / GPT-6 / DeepSeek V4 Pro）。</span>
                    </div>
                  )}

                  {/* Expanded Body Details */}
                  {isExpanded && (
                    <div className="flex flex-col gap-3 border-t border-border px-4 py-3 text-xs bg-base/50">
                      <div>
                        <span className="font-semibold text-muted">目标 (Goal): </span>
                        <span className="text-fg">{task.goal}</span>
                      </div>

                      {task.contracts.length > 0 && (
                        <div className="flex items-center gap-1.5">
                          <span className="font-semibold text-muted">引用契约: </span>
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
                          <span className="font-semibold text-muted">写入范围 (Write Scope): </span>
                          <div className="flex flex-wrap gap-1">
                            {task.write_scope.map((ws, i) => (
                              <code
                                key={i}
                                className="rounded bg-hover px-1.5 py-0.5 font-mono text-[11px] text-accent"
                              >
                                {ws}
                              </code>
                            ))}
                          </div>
                        </div>
                      )}

                      {task.deliverables.length > 0 && (
                        <div className="flex items-center gap-1.5">
                          <span className="font-semibold text-muted">预期交付物: </span>
                          <div className="flex flex-wrap gap-1">
                            {task.deliverables.map((d, i) => (
                              <span key={i} className="rounded bg-hover px-1.5 py-0.5 text-[11px] text-fg">
                                {d}
                              </span>
                            ))}
                          </div>
                        </div>
                      )}

                      {task.acceptance.commands.length > 0 && (
                        <div className="flex flex-col gap-1">
                          <span className="font-semibold text-muted">本地验收命令 (Acceptance Commands): </span>
                          <div className="flex flex-wrap gap-1.5">
                            {task.acceptance.commands.map((cmd, i) => (
                              <div
                                key={i}
                                className="flex items-center gap-1 rounded bg-elevated border border-border px-2 py-0.5 font-mono text-[11px] text-fg"
                              >
                                <Terminal className="size-3 text-muted" />
                                <code>{cmd}</code>
                              </div>
                            ))}
                          </div>
                        </div>
                      )}

                      {task.acceptance.criteria.length > 0 && (
                        <div className="flex flex-col gap-1">
                          <span className="font-semibold text-muted">自查标准 (Criteria): </span>
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
