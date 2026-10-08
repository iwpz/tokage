import { useEffect, useState } from "react";
import {
  Activity,
  AlertOctagon,
  ArrowRight,
  CheckCircle2,
  Clock,
  LayoutGrid,
  Pause,
  Play,
  RotateCcw,
  ShieldAlert,
  Table as TableIcon,
  Terminal,
  Workflow,
} from "lucide-react";
import {
  listAttempts,
  listGateRuns,
  revertOutOfScopeFiles,
  type AttemptRow,
  type GateRunRow,
  type MainSessionStatus,
  type TaskItem,
  type TaskState,
} from "../../lib/ipc";
import { useExecution } from "../../stores/execution";
import { useProviders } from "../../stores/providers";
import { useSettings } from "../../stores/settings";
import { isModelBlocked } from "../../lib/models";
import {
  cancelTask,
  completeTakeoverAndRerunGates,
  confirmContractDeviation,
  continueTask,
  dispatchReadyTasks,
  formatTaskWaitReason,
  getTaskTerminalResumeCommand,
  resolvePipelineTaskApproval,
} from "../../lib/taskRunner";
import type { DispatchIneligibleReason, UserDecision } from "../../lib/ipc";
import type { TaskApproval } from "../../stores/execution";
import type { ApprovalPart } from "../../stores/chat";
import { ApprovalCard } from "../ApprovalCard";
import { TaskModelPicker, type TreeModelGroup } from "./TasksTable";
import { usePlanSession } from "./usePlanSession";
import { Badge, Button, Modal } from "../ui";
import { cn } from "../../lib/cn";

function taskApprovalToPart(a: TaskApproval): ApprovalPart {
  return {
    type: "approval",
    id: a.id,
    turnId: a.turnId,
    toolId: a.toolId,
    command: a.command,
    level: a.level,
    reason: a.reason,
    resolved: a.resolved,
    decision: a.decision,
  };
}

export function formatPipelineDuration(startedAt?: number, endedAt?: number): string | null {
  if (!startedAt) return null;
  const elapsed = (endedAt || Date.now()) - startedAt;
  if (elapsed < 0) return null;
  const totalSec = Math.floor(elapsed / 1000);
  const min = Math.floor(totalSec / 60);
  const sec = totalSec % 60;
  if (min === 0) return `${sec}s`;
  return `${min}m ${sec}s`;
}

export function PipelineView({
  sessionId,
  onSwitchTab,
}: {
  sessionId: string;
  onSwitchTab?: (tab: any) => void;
}) {
  const { plan, loading } = usePlanSession(sessionId);
  const pipeline = useExecution((s) => s.pipelines[sessionId]);
  const updateTaskState = useExecution((s) => s.updateTaskState);
  const updateTaskAssignedModel = useExecution((s) => s.updateTaskAssignedModel);
  const appendTaskLog = useExecution((s) => s.appendTaskLog);
  const recordTaskFailureEvidence = useExecution((s) => s.recordTaskFailureEvidence);
  const pauseExecution = useExecution((s) => s.pauseExecution);
  const resumeExecution = useExecution((s) => s.resumeExecution);

  const providers = useProviders((s) => s.providers);
  const blockedModels = useSettings((s) => s.blockedModels);

  // Grouped available models for reassignment picker
  const treeGroups: TreeModelGroup[] = providers
    .filter((p) => p.chat && p.status === "READY")
    .map((p) => ({
      provider: p,
      models: (p.modelGroups || [])
        .map((g) => ({
          id: g.variants?.[0]?.id || g.key,
          name: g.name,
        }))
        .filter((m) => !isModelBlocked(m.id, blockedModels, p.id)),
    }))
    .filter((g) => g.models.length > 0);

  const [, setAttempts] = useState<AttemptRow[]>([]);
  const [, setGateRuns] = useState<GateRunRow[]>([]);
  const [selectedTask, setSelectedTask] = useState<TaskItem | null>(null);
  const [displayMode, setDisplayMode] = useState<"dag" | "table">("dag");

  // F10.e: board reads only useExecution (no parallel local state)
  const sessionStatus = (pipeline?.status as MainSessionStatus) ?? "RUNNING";
  const taskStates: Record<string, TaskState> = {
    ...(pipeline?.taskStates as Record<string, TaskState> ?? {}),
  };

  // Takeover Modal
  const [takeoverModalOpen, setTakeoverModalOpen] = useState(false);
  const [takeoverTask, setTakeoverTask] = useState<TaskItem | null>(null);
  const [resumeCmd, setResumeCmd] = useState<string | null>(null);
  const [resumeHint, setResumeHint] = useState<string | null>(null);
  const [takeoverRerunning, setTakeoverRerunning] = useState(false);

  const handleRetryTask = (task: TaskItem) => {
    // BLOCKED / INTERRUPTED → READY, then single dispatch entry (F1.h)
    updateTaskState(sessionId, task.id, "READY", `[用户操作] 重新排队任务 ${task.id}`);
    void dispatchReadyTasks(sessionId, { only: [task.id] }).catch((err) => {
      console.error(`[PipelineView] 重试任务 ${task.id} 失败:`, err);
    });
  };

  const handleRevertOutOfScope = async (task: TaskItem) => {
    const evidence = pipeline?.taskFailureEvidence?.[task.id];
    const files = evidence?.outOfScopeFiles ?? [];
    if (!evidence || files.length === 0) return;
    const ok = window.confirm(
      `将还原以下 ${files.length} 个越界文件（已跟踪文件恢复到 HEAD，未跟踪文件将被删除）：\n\n${files.join("\n")}\n\n是否继续？`,
    );
    if (!ok) return;
    try {
      const summary = await revertOutOfScopeFiles(evidence.workdir || pipeline?.cwd || ".", files);
      appendTaskLog(sessionId, task.id, `[还原越界文件] ${summary.trim() || "完成"}`);
      recordTaskFailureEvidence(sessionId, task.id, { ...evidence, outOfScopeFiles: [] });
    } catch (err) {
      appendTaskLog(sessionId, task.id, `[还原越界文件失败] ${err}`);
    }
  };

  const handleCancelTask = (task: TaskItem) => {
    void cancelTask(sessionId, task.id).catch((err) => {
      console.error(`[PipelineView] 取消任务 ${task.id} 失败:`, err);
    });
  };

  const handleContinueTask = (task: TaskItem) => {
    void continueTask(sessionId, task.id).catch((err) => {
      console.error(`[PipelineView] 继续任务 ${task.id} 失败:`, err);
    });
  };

  const handleDiscardPipeline = () => {
    useExecution.getState().discardPipeline(sessionId);
  };

  const handleResolveTaskApproval = (taskId: string, approvalId: string, decision: UserDecision) =>
    resolvePipelineTaskApproval(sessionId, taskId, approvalId, decision).catch((err) => {
      console.error(`[PipelineView] 审批 ${approvalId} 失败:`, err);
    });

  const approvalsFor = (taskId: string): TaskApproval[] =>
    pipeline?.taskApprovals?.[taskId] ?? [];

  useEffect(() => {
    const tasks = pipeline?.tasks ?? plan?.tasks;
    if (tasks && tasks.length > 0 && !selectedTask) {
      setSelectedTask(tasks[0]);
    }
  }, [pipeline?.tasks, plan?.tasks, selectedTask]);

  useEffect(() => {
    if (sessionId) {
      listAttempts(sessionId).then(setAttempts).catch(() => {});
      // F10.e: prefer planDbId when known; otherwise sessionId (no more "plan-1")
      const planKey =
        pipeline?.planDbId != null ? String(pipeline.planDbId) : String(sessionId);
      listGateRuns(undefined, planKey).then(setGateRuns).catch(() => {});
    }
  }, [sessionId, pipeline?.planDbId]);

  if (loading) {
    return (
      <div className="flex flex-1 items-center justify-center p-8 text-xs text-muted">
        正在加载流水线状态...
      </div>
    );
  }

  if (pipeline?.unrecoverable) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
        <AlertOctagon className="size-10 text-danger mb-3" />
        <h3 className="text-sm font-medium text-fg">该流水线无法恢复（计划已变化）</h3>
        <p className="mt-1 text-xs text-muted max-w-sm">
          磁盘上的执行状态与当前计划任务对不上，不能用占位任务继续调度。请丢弃后重新生成计划。
        </p>
        <Button variant="secondary" size="sm" className="mt-4" onClick={handleDiscardPipeline}>
          丢弃
        </Button>
      </div>
    );
  }

  const boardTasks = pipeline?.tasks ?? plan?.tasks ?? [];
  if (boardTasks.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
        <Workflow className="size-10 text-subtle mb-3" />
        <h3 className="text-sm font-medium text-fg">无可用流水线</h3>
        <p className="mt-1 text-xs text-muted">请先在「规划」标签页审阅并确认计划。</p>
      </div>
    );
  }

  const tasks = boardTasks;
  const runningCount = Object.values(taskStates).filter((s) => s === "RUNNING").length;
  const doneCount = Object.values(taskStates).filter((s) => s === "DONE").length;
  const blockedCount = Object.values(taskStates).filter((s) => s === "BLOCKED").length;

  const openTakeover = (task: TaskItem) => {
    setTakeoverTask(task);
    setResumeCmd(null);
    setResumeHint("正在生成续接命令…");
    setTakeoverModalOpen(true);
    void getTaskTerminalResumeCommand(sessionId, task.id).then((res) => {
      setResumeCmd(res.command);
      setResumeHint(res.hint ?? null);
    });
  };

  const handleTakeoverRerunGates = () => {
    if (!takeoverTask) return;
    setTakeoverRerunning(true);
    void completeTakeoverAndRerunGates(sessionId, takeoverTask.id)
      .catch((err) => {
        console.error(`[PipelineView] 接管重跑关卡失败:`, err);
      })
      .finally(() => {
        setTakeoverRerunning(false);
        setTakeoverModalOpen(false);
      });
  };

  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-bg">
      {/* Top Controls Bar */}
      <div className="flex items-center justify-between border-b border-border px-4 py-2.5 bg-subtle/20">
        <div className="flex items-center gap-3">
          <Workflow className="size-4 text-accent" />
          <span className="text-xs font-semibold text-fg">执行流水线看板</span>
          <Badge variant={sessionStatus === "RUNNING" ? "accent" : sessionStatus === "PAUSED" ? "warning" : "muted"}>
            {sessionStatus}
          </Badge>
          <div className="flex items-center gap-2 text-[11px] text-muted">
            <span>总计 {tasks.length}</span>
            <span>·</span>
            <span className="text-accent">运行中 {runningCount}</span>
            <span>·</span>
            <span className="text-emerald-500">完成 {doneCount}</span>
            {blockedCount > 0 && (
              <>
                <span>·</span>
                <span className="text-danger font-medium">阻塞 {blockedCount}</span>
              </>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2">
          {sessionStatus === "RUNNING" ? (
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                if (pipeline) pauseExecution(sessionId);
              }}
              className="gap-1.5 text-xs"
            >
              <Pause className="size-3.5" />
              暂停调度
            </Button>
          ) : (
            <Button
              variant="primary"
              size="sm"
              onClick={() => {
                if (pipeline) resumeExecution(sessionId);
              }}
              className="gap-1.5 text-xs"
            >
              <Play className="size-3.5" />
              恢复运行
            </Button>
          )}

          {/* View Mode Toggle: Cards / Table */}
          <div className="flex items-center rounded-md border border-border bg-base p-0.5" role="group" aria-label="视图切换">
            <button
              type="button"
              onClick={() => setDisplayMode("dag")}
              aria-pressed={displayMode === "dag"}
              title="卡片流视图"
              className={cn(
                "flex items-center gap-1 rounded px-2 py-1 text-xs transition-colors",
                displayMode === "dag" ? "bg-hover text-fg font-medium" : "text-muted hover:text-fg"
              )}
            >
              <LayoutGrid className="size-3.5" />
              <span>卡片</span>
            </button>
            <button
              type="button"
              onClick={() => setDisplayMode("table")}
              aria-pressed={displayMode === "table"}
              title="等价表格视图（读屏与无障碍兼容）"
              className={cn(
                "flex items-center gap-1 rounded px-2 py-1 text-xs transition-colors",
                displayMode === "table" ? "bg-hover text-fg font-medium" : "text-muted hover:text-fg"
              )}
            >
              <TableIcon className="size-3.5" />
              <span>表格</span>
            </button>
          </div>

          <Button
            variant="ghost"
            size="sm"
            onClick={() => onSwitchTab?.("plan")}
            className="text-xs text-muted hover:text-fg"
          >
            返回审阅
          </Button>
        </div>
      </div>

      {/* F9.e: contract deviation pause banner */}
      {(pipeline?.deviationSources?.length ?? 0) > 0 && (
        <div className="border-b border-warning/40 bg-warning/10 px-4 py-2.5 space-y-2">
          {pipeline!.deviationSources!.map((srcId) => {
            const srcTask = tasks.find((t) => t.id === srcId);
            return (
              <div key={srcId} className="flex items-start justify-between gap-3 text-xs">
                <div className="min-w-0">
                  <span className="font-medium text-warning">契约偏离待确认</span>
                  <span className="text-muted"> · {srcId}</span>
                  {srcTask && (
                    <span className="text-muted"> {srcTask.title}</span>
                  )}
                  <p className="mt-0.5 text-muted">
                    下游任务已暂停调度。确认偏离可接受后继续执行。
                  </p>
                </div>
                <Button
                  variant="primary"
                  size="sm"
                  className="shrink-0 h-7 text-[11px]"
                  onClick={() => {
                    void confirmContractDeviation(sessionId, srcId).catch((err) => {
                      console.error(`[PipelineView] 确认偏离 ${srcId} 失败:`, err);
                    });
                  }}
                >
                  确认继续
                </Button>
              </div>
            );
          })}
        </div>
      )}

      {/* Main Split: Task Flow (Cards or Table) (Left) & Inspector (Right) */}
      <div className="flex flex-1 min-h-0 overflow-hidden">
        {/* Left: Task Flow Cards or Table */}
        <div className="flex-1 overflow-y-auto p-4 space-y-3 border-r border-border">
          {displayMode === "table" ? (
            <div className="overflow-x-auto rounded-lg border border-border bg-card">
              <table className="w-full text-left text-xs border-collapse" aria-label="流水线任务表格">
                <thead>
                  <tr className="border-b border-border bg-subtle/30 text-muted font-medium">
                    <th scope="col" className="p-2.5">ID</th>
                    <th scope="col" className="p-2.5">状态</th>
                    <th scope="col" className="p-2.5">标题</th>
                    <th scope="col" className="p-2.5">领域</th>
                    <th scope="col" className="p-2.5">前置依赖</th>
                    <th scope="col" className="p-2.5">指派模型</th>
                    <th scope="col" className="p-2.5">耗时</th>
                    <th scope="col" className="p-2.5 text-right">操作</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/60">
                  {tasks.map((task) => {
                    const state = taskStates[task.id] || "PENDING";
                    const isSelected = selectedTask?.id === task.id;
                    const waitReason = pipeline?.taskWaitReasons?.[task.id] as
                      | DispatchIneligibleReason
                      | undefined;
                    const durationText = formatPipelineDuration(
                      pipeline?.taskStartedAt?.[task.id],
                      pipeline?.taskEndedAt?.[task.id]
                    );
                    return (
                      <tr
                        key={task.id}
                        tabIndex={0}
                        onClick={() => setSelectedTask(task)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            setSelectedTask(task);
                          }
                        }}
                        className={cn(
                          "cursor-pointer transition-colors hover:bg-hover/40",
                          isSelected && "bg-accent/10"
                        )}
                      >
                        <td className="p-2.5 font-mono font-bold text-fg">{task.id}</td>
                        <td className="p-2.5">
                          <div className="flex flex-col gap-0.5">
                            <TaskStateBadge state={state} />
                            {state === "READY" && waitReason && (
                              <span className="text-[10px] text-warning leading-tight">
                                {formatTaskWaitReason(waitReason)}
                              </span>
                            )}
                          </div>
                        </td>
                        <td className="p-2.5 font-medium text-fg max-w-xs truncate">{task.title}</td>
                        <td className="p-2.5">
                          <Badge variant="muted" className="text-[10px] uppercase font-mono">
                            {task.domain}
                          </Badge>
                        </td>
                        <td className="p-2.5 text-muted font-mono text-[11px]">
                          {task.depends_on.length > 0 ? task.depends_on.join(", ") : "—"}
                        </td>
                        <td className="p-2.5 text-muted font-mono text-[11px] truncate max-w-[120px]">
                          {task.assigned_model || "默认"}
                        </td>
                        <td className="p-2.5 text-muted font-mono text-[11px] tabular-nums">
                          {durationText || "—"}
                        </td>
                        <td className="p-2.5 text-right">
                          <div className="flex items-center justify-end gap-1.5" onClick={(e) => e.stopPropagation()}>
                            {state === "READY" && (
                              <Button
                                variant="secondary"
                                size="sm"
                                className="h-6 text-[11px] px-2"
                                onClick={() => handleRetryTask(task)}
                              >
                                执行
                              </Button>
                            )}
                            {state === "INTERRUPTED" && (
                              <Button
                                variant="primary"
                                size="sm"
                                className="h-6 text-[11px] px-2"
                                onClick={() => handleContinueTask(task)}
                              >
                                继续
                              </Button>
                            )}
                            {state === "BLOCKED" && (
                              <>
                                <Button
                                  variant="secondary"
                                  size="sm"
                                  className="h-6 text-[11px] px-2 text-accent"
                                  onClick={() => handleRetryTask(task)}
                                >
                                  重跑
                                </Button>
                                {(pipeline?.taskFailureEvidence?.[task.id]?.outOfScopeFiles.length ?? 0) > 0 && (
                                  <Button
                                    variant="secondary"
                                    size="sm"
                                    className="h-6 text-[11px] px-2 text-danger hover:text-danger"
                                    onClick={() => void handleRevertOutOfScope(task)}
                                  >
                                    还原越界文件
                                  </Button>
                                )}
                                <Button
                                  variant="secondary"
                                  size="sm"
                                  className="h-6 text-[11px] px-2 text-danger hover:text-danger"
                                  onClick={() => openTakeover(task)}
                                >
                                  接管
                                </Button>
                              </>
                            )}
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="grid grid-cols-1 gap-2.5">
            {tasks.map((task) => {
              const state = taskStates[task.id] || "PENDING";
              const isSelected = selectedTask?.id === task.id;
              const waitReason = pipeline?.taskWaitReasons?.[task.id] as
                | DispatchIneligibleReason
                | undefined;
              const durationText = formatPipelineDuration(
                pipeline?.taskStartedAt?.[task.id],
                pipeline?.taskEndedAt?.[task.id]
              );

              return (
                <div
                  key={task.id}
                  onClick={() => setSelectedTask(task)}
                  className={cn(
                    "flex flex-col p-3 rounded-lg border transition-all cursor-pointer",
                    isSelected
                      ? "border-accent bg-accent/5 shadow-xs"
                      : "border-border bg-card hover:border-border/80 hover:bg-hover/30",
                    state === "BLOCKED" && "border-danger/60 bg-danger/5"
                  )}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-xs font-bold text-fg">{task.id}</span>
                      <span className="text-xs font-medium text-fg truncate max-w-sm">{task.title}</span>
                      <Badge variant="muted" className="text-[10px] uppercase font-mono">
                        {task.domain}
                      </Badge>
                      {task.assigned_model && (
                        <span className="rounded bg-elevated px-1.5 py-0.5 text-[10px] font-mono text-subtle border border-border/60">
                          {task.assigned_model}
                        </span>
                      )}
                    </div>

                    <div className="flex items-center gap-2">
                      {durationText && (
                        <span className="font-mono text-[11px] tabular-nums text-subtle">
                          {durationText}
                        </span>
                      )}
                      <TaskStateBadge state={state} />
                    </div>
                  </div>

                  {state === "READY" && waitReason && (
                    <p className="mt-1.5 text-[11px] text-warning">
                      {formatTaskWaitReason(waitReason)}
                    </p>
                  )}

                  <div className="mt-2 flex items-center justify-between text-[11px] text-muted">
                    <div className="flex items-center gap-2">
                      <span>依赖: {task.depends_on.length > 0 ? task.depends_on.join(", ") : "无"}</span>
                      {task.write_scope.length > 0 && (
                        <span>· 作用域: {task.write_scope[0]}</span>
                      )}
                    </div>

                    {/* Quick Inline Actions */}
                    <div className="flex items-center gap-1.5" onClick={(e) => e.stopPropagation()}>
                      {state === "READY" && (
                        <Button
                          variant="secondary"
                          size="sm"
                          className="h-6 text-[11px] px-2"
                          onClick={() => handleRetryTask(task)}
                        >
                          开始执行
                        </Button>
                      )}
                      {(state === "RUNNING" || state === "VERIFYING") && (
                        <Button
                          variant="secondary"
                          size="sm"
                          className="h-6 text-[11px] px-2 border-danger text-danger hover:bg-danger/10"
                          onClick={() => handleCancelTask(task)}
                        >
                          取消任务
                        </Button>
                      )}
                      {state === "INTERRUPTED" && (
                        <Button
                          variant="primary"
                          size="sm"
                          className="h-6 text-[11px] px-2"
                          onClick={() => handleContinueTask(task)}
                        >
                          继续
                        </Button>
                      )}
                      {state === "BLOCKED" && (
                        <>
                          <Button
                            variant="secondary"
                            size="sm"
                            className="h-6 text-[11px] px-2 text-accent"
                            onClick={() => handleRetryTask(task)}
                          >
                            重新执行
                          </Button>
                          {(pipeline?.taskFailureEvidence?.[task.id]?.outOfScopeFiles.length ?? 0) > 0 && (
                            <Button
                              variant="secondary"
                              size="sm"
                              className="h-6 text-[11px] px-2 border-danger text-danger hover:bg-danger/10"
                              onClick={() => void handleRevertOutOfScope(task)}
                            >
                              还原越界文件
                            </Button>
                          )}
                          <Button
                            variant="secondary"
                            size="sm"
                            className="h-6 text-[11px] px-2 gap-1 border-danger text-danger hover:bg-danger/10"
                            onClick={() => openTakeover(task)}
                          >
                            <Terminal className="size-3" />
                            终端接管
                          </Button>
                        </>
                      )}
                    </div>
                  </div>

                  {/* F6.c: pending approvals on the card */}
                  {approvalsFor(task.id).some((a) => !a.resolved) && (
                    <div className="mt-2 space-y-1" onClick={(e) => e.stopPropagation()}>
                      {approvalsFor(task.id)
                        .filter((a) => !a.resolved)
                        .map((a) => (
                          <ApprovalCard
                            key={a.id}
                            p={taskApprovalToPart(a)}
                            sessionId={sessionId}
                            onResolve={(id, decision) => handleResolveTaskApproval(task.id, id, decision)}
                          />
                        ))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
          )}
        </div>

        {/* Right: Task Detail Inspector */}
        <div className="w-96 flex flex-col overflow-y-auto p-4 bg-elevated/30 space-y-4">
          {selectedTask ? (
            <>
              <div>
                <div className="flex items-center justify-between">
                  <span className="font-mono text-xs font-bold text-fg">{selectedTask.id}</span>
                  <TaskStateBadge state={taskStates[selectedTask.id] || "PENDING"} />
                </div>
                <h4 className="mt-1 text-sm font-semibold text-fg">{selectedTask.title}</h4>
                <p className="mt-1 text-xs text-muted leading-relaxed">
                  {selectedTask.goal || "暂无具体目标描述"}
                </p>
              </div>

              {/* Model assignment & Duration */}
              <div className="rounded-md border border-border p-3 bg-subtle/10 space-y-2">
                <span className="text-xs font-semibold text-fg">执行代理配置与耗时</span>
                <div className="space-y-2 text-xs">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-muted">指派模型:</span>
                    <TaskModelPicker
                      value={selectedTask.assigned_model}
                      placeholder="选择执行模型"
                      treeGroups={treeGroups}
                      onChange={(newModel) => {
                        updateTaskAssignedModel(sessionId, selectedTask.id, newModel);
                        setSelectedTask({ ...selectedTask, assigned_model: newModel });
                      }}
                      onReset={() => {
                        updateTaskAssignedModel(sessionId, selectedTask.id, "");
                        setSelectedTask({ ...selectedTask, assigned_model: undefined });
                      }}
                    />
                  </div>
                  {formatPipelineDuration(
                    pipeline?.taskStartedAt?.[selectedTask.id],
                    pipeline?.taskEndedAt?.[selectedTask.id]
                  ) && (
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-muted">执行耗时:</span>
                      <span className="font-mono text-[11px] tabular-nums text-fg">
                        {formatPipelineDuration(
                          pipeline?.taskStartedAt?.[selectedTask.id],
                          pipeline?.taskEndedAt?.[selectedTask.id]
                        )}
                      </span>
                    </div>
                  )}
                </div>
              </div>

              {/* Scope & Dependencies */}
              <div className="rounded-md border border-border p-3 bg-subtle/10 space-y-2">
                <span className="text-xs font-semibold text-fg">执行约束</span>
                <div className="space-y-1.5 text-xs">
                  <div>
                    <span className="text-muted">写入范围:</span>
                    <div className="mt-0.5 flex flex-wrap gap-1">
                      {selectedTask.write_scope.map((s, i) => (
                        <span key={i} className="font-mono text-[11px] bg-bg px-1.5 py-0.5 rounded border border-border text-fg">
                          {s}
                        </span>
                      ))}
                    </div>
                  </div>
                  {selectedTask.deliverables && selectedTask.deliverables.length > 0 && (
                    <div>
                      <span className="text-muted">交付物:</span>
                      <div className="mt-0.5 flex flex-wrap gap-1">
                        {selectedTask.deliverables.map((s, i) => (
                          <span key={i} className="font-mono text-[11px] bg-bg px-1.5 py-0.5 rounded border border-border text-fg">
                            {s}
                          </span>
                        ))}
                      </div>
                    </div>
                  )}
                  <div>
                    <span className="text-muted">上游前置依赖:</span>
                    <div className="mt-0.5 font-mono text-[11px] text-fg">
                      {selectedTask.depends_on.length > 0 ? selectedTask.depends_on.join(", ") : "无前置依赖"}
                    </div>
                  </div>
                </div>
              </div>

              {/* Acceptance Gates & Verification */}
              <div className="rounded-md border border-border p-3 bg-subtle/10 space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-semibold text-fg flex items-center gap-1.5">
                    <CheckCircle2 className="size-3.5 text-accent" />
                    自动化验收关卡
                  </span>
                </div>

                {selectedTask.acceptance.commands.length > 0 ? (
                  <div className="space-y-1">
                    {selectedTask.acceptance.commands.map((cmd, i) => (
                      <div key={i} className="font-mono text-[11px] bg-bg px-2 py-1 rounded border border-border text-fg">
                        $ {cmd}
                      </div>
                    ))}
                  </div>
                ) : (
                  <span className="text-[11px] text-subtle">无预置自动化验收命令</span>
                )}

                {pipeline?.gateResults && pipeline.gateResults.filter((g) => g.taskId === selectedTask.id).length > 0 && (
                  <div className="mt-2 space-y-1.5 pt-2 border-t border-border/40">
                    <span className="text-[11px] font-medium text-fg">历史验收执行记录:</span>
                    {pipeline.gateResults
                      .filter((g) => g.taskId === selectedTask.id)
                      .map((gr, idx) => (
                        <div key={idx} className="rounded border border-border/80 bg-bg p-2 text-[11px] font-mono">
                          <div className="flex items-center justify-between">
                            <span className="text-fg font-semibold truncate max-w-[180px]">{gr.command}</span>
                            <span className={gr.passed ? "text-emerald-400" : "text-rose-400"}>
                              {gr.passed ? "✓ 通过" : `✗ 失败 (${gr.exitCode ?? "?"})`}
                            </span>
                          </div>
                          <div className="mt-0.5 text-muted text-[10px]">
                            耗时: {gr.durationMs}ms
                          </div>
                          {gr.outputTail && (
                            <pre className="mt-1 max-h-24 overflow-y-auto rounded bg-card p-1 text-[10px] text-muted whitespace-pre-wrap leading-tight">
                              {gr.outputTail}
                            </pre>
                          )}
                        </div>
                      ))}
                  </div>
                )}
              </div>

              {/* F6.c: task approvals in inspector */}
              {approvalsFor(selectedTask.id).length > 0 && (
                <div className="rounded-md border border-border p-3 bg-subtle/10 space-y-2">
                  <span className="text-xs font-semibold text-fg flex items-center gap-1.5">
                    <ShieldAlert className="size-3.5 text-warning" />
                    待审批命令
                  </span>
                  {approvalsFor(selectedTask.id).map((a) => (
                    <ApprovalCard
                      key={a.id}
                      p={taskApprovalToPart(a)}
                      sessionId={sessionId}
                      onResolve={(id, decision) =>
                        handleResolveTaskApproval(selectedTask.id, id, decision)
                      }
                    />
                  ))}
                  {!pipeline?.threadIds?.[selectedTask.id] &&
                    approvalsFor(selectedTask.id).some((a) => !a.resolved) && (
                      <p className="text-[11px] text-warning">
                        无 CLI 会话 ID，批准后无法自动续接；请使用「重新执行」。
                      </p>
                    )}
                </div>
              )}

              {/* Real-time Sub-agent Execution Logs */}
              {pipeline?.taskLogs?.[selectedTask.id] && pipeline.taskLogs[selectedTask.id].length > 0 && (
                <div className="rounded-md border border-border p-3 bg-subtle/10 space-y-2">
                  <span className="text-xs font-semibold text-fg flex items-center gap-1.5">
                    <Terminal className="size-3.5 text-accent" />
                    子代理执行日志
                  </span>
                  <pre
                    data-selectable
                    className="max-h-56 overflow-y-auto rounded bg-bg p-2 text-[11px] font-mono text-muted whitespace-pre-wrap leading-relaxed border border-border/60"
                  >
                    {pipeline.taskLogs[selectedTask.id].join("\n")}
                  </pre>
                </div>
              )}

            </>
          ) : (
            <div className="flex flex-1 items-center justify-center text-xs text-subtle">
              请选择一个任务查看详情
            </div>
          )}
        </div>
      </div>

      {/* Terminal Takeover Modal (F10.c) */}
      <Modal
        open={takeoverModalOpen}
        onClose={() => setTakeoverModalOpen(false)}
        title={`终端接管任务: ${takeoverTask?.id}`}
      >
        <div className="space-y-4 text-xs">
          <p className="text-muted leading-relaxed">
            该任务当前为 <strong>BLOCKED</strong>。可在外部终端续接 CLI 会话，人工调试并修改代码：
          </p>
          {resumeCmd ? (
            <div className="bg-bg p-3 rounded-md font-mono text-[12px] border border-border text-fg select-all">
              {resumeCmd}
            </div>
          ) : (
            <div className="bg-bg p-3 rounded-md text-[12px] border border-border text-warning">
              {resumeHint || "无可续接会话"}
            </div>
          )}
          <p className="text-[11px] text-muted">
            在外部终端完成修改后，点击下方「已完成接管，重跑关卡」，Tokage 将按任务目录执行全部验收命令。
          </p>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={() => setTakeoverModalOpen(false)}>
              稍后处理
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                if (takeoverTask) {
                  handleRetryTask(takeoverTask);
                }
                setTakeoverModalOpen(false);
              }}
            >
              重新唤起子代理
            </Button>
            <Button
              variant="primary"
              disabled={takeoverRerunning}
              onClick={handleTakeoverRerunGates}
            >
              {takeoverRerunning ? "重跑中…" : "已完成接管，重跑关卡"}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}

function TaskStateBadge({ state }: { state: TaskState }) {
  switch (state) {
    case "PENDING":
      return (
        <Badge variant="muted" className="gap-1 text-[10px]">
          <Clock className="size-3 text-subtle" />
          PENDING
        </Badge>
      );
    case "READY":
      return (
        <Badge variant="accent" className="gap-1 text-[10px]">
          <ArrowRight className="size-3 text-accent" />
          READY
        </Badge>
      );
    case "RUNNING":
      return (
        <Badge variant="accent" className="gap-1 text-[10px] animate-pulse">
          <Play className="size-3 fill-accent text-accent" />
          RUNNING
        </Badge>
      );
    case "VERIFYING":
      return (
        <Badge variant="warning" className="gap-1 text-[10px]">
          <Activity className="size-3" />
          VERIFYING
        </Badge>
      );
    case "DONE":
      return (
        <Badge variant="success" className="gap-1 text-[10px]">
          <CheckCircle2 className="size-3 text-emerald-500" />
          DONE
        </Badge>
      );
    case "REPAIRING":
      return (
        <Badge variant="warning" className="gap-1 text-[10px]">
          <RotateCcw className="size-3 text-amber-500" />
          REPAIRING
        </Badge>
      );
    case "BLOCKED":
      return (
        <Badge variant="danger" className="gap-1 text-[10px]">
          <AlertOctagon className="size-3 text-danger" />
          BLOCKED
        </Badge>
      );
    case "PAUSED_QUOTA":
      return (
        <Badge variant="danger" className="gap-1 text-[10px]">
          <Pause className="size-3 text-rose-400" />
          配额熔断 (PAUSED)
        </Badge>
      );
    case "WAITING_UPSTREAM":
      return (
        <Badge variant="muted" className="gap-1 text-[10px]">
          <Clock className="size-3 text-muted" />
          等待上游恢复
        </Badge>
      );
    case "INTERRUPTED":
      return (
        <Badge variant="warning" className="gap-1 text-[10px]">
          <AlertOctagon className="size-3 text-amber-500" />
          INTERRUPTED
        </Badge>
      );
    default:
      return <Badge variant="muted">{state}</Badge>;
  }
}
