import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import i18n from "../../i18n";
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
  Square,
} from "lucide-react";
import {
  listAttempts,
  listGateRuns,
  revertOutOfScopeFiles,
  type AttemptRow,
  type GateRunRow,
  type PipelineStatus,
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
  continueInterrupted,
  continueTask,
  interruptExecution,
  deliverExportPatch,
  deliverKeepBranch,
  deliverMergeToCheckout,
  dispatchReadyTasks,
  formatTaskWaitReason,
  getTaskTerminalResumeCommand,
  maybeStartPlanDelivery,
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
import { collectUnifiedApprovals } from "../../lib/unifiedApprovals";
import { renderLogMsg, renderLogMsgs } from "../../lib/logMsg";

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
  const { t } = useTranslation();
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
  const banningLevel = useSettings((s) => s.modelBanningLevel);
  const modelProviderPreferences = useSettings((s) => s.modelProviderPreferences);
  const modelChannelAccess = useSettings((s) => s.modelChannelAccess);
  // Must stay above early returns — conditional hooks crash React.
  const isolation = useSettings((s) => s.isolation);

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
        .filter(
          (m) =>
            !isModelBlocked(
              m.id,
              blockedModels,
              p.id,
              banningLevel,
              modelProviderPreferences,
              modelChannelAccess,
            ),
        ),
    }))
    .filter((g) => g.models.length > 0);

  const [, setAttempts] = useState<AttemptRow[]>([]);
  const [, setGateRuns] = useState<GateRunRow[]>([]);
  const [selectedTask, setSelectedTask] = useState<TaskItem | null>(null);
  const [displayMode, setDisplayMode] = useState<"dag" | "table">("dag");

  // F10.e: board reads only useExecution (no parallel local state)
  const sessionStatus: PipelineStatus = pipeline?.status ?? "RUNNING";
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
    updateTaskState(sessionId, task.id, "READY", i18n.t("pipeline.logRequeue", { id: task.id }));
    void dispatchReadyTasks(sessionId, { only: [task.id] }).catch((err) => {
      console.error(`[PipelineView] Retry task ${task.id} failed:`, err);
    });
  };

  const handleRevertOutOfScope = async (task: TaskItem) => {
    const evidence = pipeline?.taskFailureEvidence?.[task.id];
    const files = evidence?.outOfScopeFiles ?? [];
    if (!evidence || files.length === 0) return;
    const ok = window.confirm(
      i18n.t("pipeline.revertConfirm", { count: files.length, files: files.join("\n") }),
    );
    if (!ok) return;
    try {
      const summary = await revertOutOfScopeFiles(evidence.workdir || pipeline?.cwd || ".", files);
      appendTaskLog(sessionId, task.id, i18n.t("pipeline.logRevert", { summary: summary.trim() || i18n.t("pipeline.logRevertDone") }));
      recordTaskFailureEvidence(sessionId, task.id, { ...evidence, outOfScopeFiles: [] });
    } catch (err) {
      appendTaskLog(sessionId, task.id, i18n.t("pipeline.logRevertFailed", { error: String(err) }));
    }
  };

  const handleCancelTask = (task: TaskItem) => {
    void cancelTask(sessionId, task.id).catch((err) => {
      console.error(`[PipelineView] Cancel task ${task.id} failed:`, err);
    });
  };

  const handleContinueTask = (task: TaskItem) => {
    void continueTask(sessionId, task.id).catch((err) => {
      console.error(`[PipelineView] Continue task ${task.id} failed:`, err);
    });
  };

  const handleDiscardPipeline = () => {
    useExecution.getState().discardPipeline(sessionId);
  };

  const handleResolveTaskApproval = (taskId: string, approvalId: string, decision: UserDecision) =>
    resolvePipelineTaskApproval(sessionId, taskId, approvalId, decision).catch((err) => {
      console.error(`[PipelineView] Approval ${approvalId} failed:`, err);
    });

  const approvalsFor = (taskId: string): TaskApproval[] =>
    pipeline?.taskApprovals?.[taskId] ?? [];

  const unifiedApprovals = collectUnifiedApprovals(pipeline);

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

  // F11.f: kick off delivery when the board opens on an already-DONE pipeline
  useEffect(() => {
    if (pipeline?.status === "DONE" && !pipeline.delivery) {
      void maybeStartPlanDelivery(sessionId);
    }
  }, [sessionId, pipeline?.status, pipeline?.delivery]);

  if (loading) {
    return (
      <div className="flex flex-1 items-center justify-center p-8 text-xs text-muted">
        {t("pipeline.loading")}
      </div>
    );
  }

  if (pipeline?.unrecoverable) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
        <AlertOctagon className="size-10 text-danger mb-3" />
        <h3 className="text-sm font-medium text-fg">{t("pipeline.unrecoverableTitle")}</h3>
        <p className="mt-1 text-xs text-muted max-w-sm">
          {t("pipeline.unrecoverableBody")}
        </p>
        <Button variant="secondary" size="sm" className="mt-4" onClick={handleDiscardPipeline}>
          {t("pipeline.discard")}
        </Button>
      </div>
    );
  }

  const boardTasks = pipeline?.tasks ?? plan?.tasks ?? [];
  if (boardTasks.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
        <Workflow className="size-10 text-subtle mb-3" />
        <h3 className="text-sm font-medium text-fg">{t("pipeline.emptyTitle")}</h3>
        <p className="mt-1 text-xs text-muted">{t("pipeline.emptyBody")}</p>
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
    setResumeHint(t("pipeline.generatingResume"));
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
        console.error(`[PipelineView] Takeover re-run gates failed:`, err);
      })
      .finally(() => {
        setTakeoverRerunning(false);
        setTakeoverModalOpen(false);
      });
  };

  const delivery = pipeline?.delivery ?? null;
  const showDelivery =
    sessionStatus === "DONE" &&
    (delivery != null || (isolation === "worktree" && !!pipeline?.integration));

  const handleDeliveryMerge = () => {
    const ok = window.confirm(i18n.t("pipeline.mergeConfirm"));
    if (!ok) return;
    void deliverMergeToCheckout(sessionId);
  };

  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-bg">
      {showDelivery && (
        <div className="border-b border-border bg-subtle/30 px-4 py-3">
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0 flex-1">
              <div className="text-xs font-semibold text-fg">{t("pipeline.delivery.title")}</div>
              {delivery?.phase === "running_gates" && (
                <p className="mt-1 text-[11px] text-muted">{t("pipeline.delivery.runningGates")}</p>
              )}
              {delivery?.phase === "failed" && (
                <p className="mt-1 text-[11px] text-danger">{renderLogMsg(delivery.error ?? "")}</p>
              )}
              {(delivery?.phase === "ready" ||
                delivery?.phase === "merged" ||
                delivery?.phase === "exported" ||
                delivery?.phase === "kept") &&
                delivery.summary && (
                  <div className="mt-1 space-y-0.5 text-[11px] text-muted">
                    <p>
                      {t("pipeline.delivery.summary", {
                        completed: delivery.summary.completedTasks,
                        total: delivery.summary.totalTasks,
                        files: delivery.summary.touchedFiles.length,
                        cost: delivery.summary.actualCostUsd.toFixed(2),
                      })}
                      {delivery.fullGatePassed === false && (
                        <span className="text-warning">{t("pipeline.delivery.fullGateFailed")}</span>
                      )}
                    </p>
                    {delivery.error && (
                      <p className="text-warning">{renderLogMsg(delivery.error ?? "")}</p>
                    )}
                    {delivery.patchPath && (
                      <p className="font-mono text-fg/80">{t("pipeline.delivery.patch", { path: delivery.patchPath })}</p>
                    )}
                    {delivery.phase === "merged" && (
                      <p className="text-emerald-600">{t("pipeline.delivery.merged")}</p>
                    )}
                    {delivery.phase === "kept" && (
                      <p className="text-fg/80">{t("pipeline.delivery.kept")}</p>
                    )}
                  </div>
                )}
              {!delivery && (
                <p className="mt-1 text-[11px] text-muted">{t("pipeline.delivery.preparing")}</p>
              )}
            </div>
            {isolation === "worktree" &&
              pipeline?.integration &&
              (delivery?.phase === "ready" || !delivery) && (
                <div className="flex shrink-0 flex-wrap gap-1.5">
                  <Button variant="primary" size="sm" className="text-xs" onClick={handleDeliveryMerge}>
                    {t("pipeline.delivery.merge")}
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    className="text-xs"
                    onClick={() => void deliverExportPatch(sessionId)}
                  >
                    {t("pipeline.delivery.exportPatch")}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-xs"
                    onClick={() => void deliverKeepBranch(sessionId)}
                  >
                    {t("pipeline.delivery.keepBranch")}
                  </Button>
                </div>
              )}
          </div>
        </div>
      )}
      {/* Top Controls Bar */}
      <div className="flex items-center justify-between border-b border-border px-4 py-2.5 bg-subtle/20">
        <div className="flex items-center gap-3">
          <Workflow className="size-4 text-accent" />
          <span className="text-xs font-semibold text-fg">{t("pipeline.boardTitle")}</span>
          <Badge variant={sessionStatus === "RUNNING" ? "accent" : sessionStatus === "PAUSED" ? "warning" : "muted"}>
            {sessionStatus}
          </Badge>
          <div className="flex items-center gap-2 text-[11px] text-muted">
            <span>{t("pipeline.statsTotal", { count: tasks.length })}</span>
            <span>·</span>
            <span className="text-accent">{t("pipeline.statsRunning", { count: runningCount })}</span>
            <span>·</span>
            <span className="text-emerald-500">{t("pipeline.statsDone", { count: doneCount })}</span>
            {blockedCount > 0 && (
              <>
                <span>·</span>
                <span className="text-danger font-medium">{t("pipeline.statsBlocked", { count: blockedCount })}</span>
              </>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2">
          {sessionStatus === "RUNNING" && (
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void interruptExecution(sessionId)}
              className="gap-1.5 text-xs"
            >
              <Square className="size-3.5" />
              {t("pipeline.interrupt")}
            </Button>
          )}
          {sessionStatus === "INTERRUPTED" ? (
            <Button
              variant="primary"
              size="sm"
              onClick={() => void continueInterrupted(sessionId)}
              className="gap-1.5 text-xs"
            >
              <Play className="size-3.5" />
              {t("pipeline.continueAll")}
            </Button>
          ) : sessionStatus === "RUNNING" ? (
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                if (pipeline) pauseExecution(sessionId);
              }}
              className="gap-1.5 text-xs"
            >
              <Pause className="size-3.5" />
              {t("pipeline.pauseDispatch")}
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
              {t("pipeline.resumeDispatch")}
            </Button>
          )}

          {/* View Mode Toggle: Cards / Table */}
          <div className="flex items-center rounded-md border border-border bg-base p-0.5" role="group" aria-label={t("pipeline.viewToggleAria")}>
            <button
              type="button"
              onClick={() => setDisplayMode("dag")}
              aria-pressed={displayMode === "dag"}
              title={t("pipeline.viewCardsTitle")}
              className={cn(
                "flex items-center gap-1 rounded px-2 py-1 text-xs transition-colors",
                displayMode === "dag" ? "bg-hover text-fg font-medium" : "text-muted hover:text-fg"
              )}
            >
              <LayoutGrid className="size-3.5" />
              <span>{t("pipeline.viewCards")}</span>
            </button>
            <button
              type="button"
              onClick={() => setDisplayMode("table")}
              aria-pressed={displayMode === "table"}
              title={t("pipeline.viewTableTitle")}
              className={cn(
                "flex items-center gap-1 rounded px-2 py-1 text-xs transition-colors",
                displayMode === "table" ? "bg-hover text-fg font-medium" : "text-muted hover:text-fg"
              )}
            >
              <TableIcon className="size-3.5" />
              <span>{t("pipeline.viewTable")}</span>
            </button>
          </div>

          <Button
            variant="ghost"
            size="sm"
            onClick={() => onSwitchTab?.("plan")}
            className="text-xs text-muted hover:text-fg"
          >
            {t("pipeline.backToReview")}
          </Button>
        </div>
      </div>

      {/* F9.e: contract deviation pause banner */}
      {(pipeline?.deviationSources?.length ?? 0) > 0 && (
        <div className="border-b border-warning/40 bg-warning/10 px-4 py-2.5 space-y-2">
          {pipeline!.deviationSources!.map((srcId) => {
            const srcTask = tasks.find((task) => task.id === srcId);
            return (
              <div key={srcId} className="flex items-start justify-between gap-3 text-xs">
                <div className="min-w-0">
                  <span className="font-medium text-warning">{t("pipeline.deviationTitle")}</span>
                  <span className="text-muted"> · {srcId}</span>
                  {srcTask && (
                    <span className="text-muted"> {srcTask.title}</span>
                  )}
                  <p className="mt-0.5 text-muted">
                    {t("pipeline.deviationBody")}
                  </p>
                </div>
                <Button
                  variant="primary"
                  size="sm"
                  className="shrink-0 h-7 text-[11px]"
                  onClick={() => {
                    void confirmContractDeviation(sessionId, srcId).catch((err) => {
                      console.error(`[PipelineView] Confirm deviation ${srcId} failed:`, err);
                    });
                  }}
                >
                  {t("pipeline.deviationConfirm")}
                </Button>
              </div>
            );
          })}
        </div>
      )}

      {/* Unified approval queue (docs/07 §5) — session-wide pending approvals */}
      {unifiedApprovals.length > 0 && (
        <div className="border-b border-accent/30 bg-accent/5 px-4 py-2.5 space-y-2">
          <div className="flex items-center gap-2 text-xs">
            <ShieldAlert className="size-3.5 text-accent" />
            <span className="font-medium text-fg">{t("pipeline.approvalQueue")}</span>
            <span className="text-muted">{t("pipeline.approvalPending", { count: unifiedApprovals.length })}</span>
          </div>
          <div className="space-y-1.5">
            {unifiedApprovals.map((item) => (
              <div key={item.approvalId} className="flex items-start gap-2">
                <span className="shrink-0 font-mono text-[10px] text-muted pt-1.5">
                  {item.taskId}
                </span>
                <div className="min-w-0 flex-1" onClick={(e) => e.stopPropagation()}>
                  <ApprovalCard
                    p={taskApprovalToPart(item.source)}
                    sessionId={sessionId}
                    onResolve={(id, decision) =>
                      handleResolveTaskApproval(item.taskId, id, decision)
                    }
                  />
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Main Split: Task Flow (Cards or Table) (Left) & Inspector (Right) */}
      <div className="flex flex-1 min-h-0 overflow-hidden">
        {/* Left: Task Flow Cards or Table */}
        <div className="flex-1 overflow-y-auto p-4 space-y-3 border-r border-border">
          {displayMode === "table" ? (
            <div className="overflow-x-auto rounded-lg border border-border bg-card">
              <table className="w-full text-left text-xs border-collapse" aria-label={t("pipeline.tableAria")}>
                <thead>
                  <tr className="border-b border-border bg-subtle/30 text-muted font-medium">
                    <th scope="col" className="p-2.5">ID</th>
                    <th scope="col" className="p-2.5">{t("pipeline.colStatus")}</th>
                    <th scope="col" className="p-2.5">{t("pipeline.colTitle")}</th>
                    <th scope="col" className="p-2.5">{t("pipeline.colDomain")}</th>
                    <th scope="col" className="p-2.5">{t("pipeline.colDepends")}</th>
                    <th scope="col" className="p-2.5">{t("pipeline.colModel")}</th>
                    <th scope="col" className="p-2.5">{t("pipeline.colDuration")}</th>
                    <th scope="col" className="p-2.5 text-right">{t("pipeline.colActions")}</th>
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
                                {renderLogMsg(formatTaskWaitReason(waitReason))}
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
                          {task.assigned_model || t("pipeline.defaultModel")}
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
                                {t("pipeline.run")}
                              </Button>
                            )}
                            {state === "INTERRUPTED" && (
                              <Button
                                variant="primary"
                                size="sm"
                                className="h-6 text-[11px] px-2"
                                onClick={() => handleContinueTask(task)}
                              >
                                {t("pipeline.continue")}
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
                                  {t("pipeline.rerun")}
                                </Button>
                                {(pipeline?.taskFailureEvidence?.[task.id]?.outOfScopeFiles.length ?? 0) > 0 && (
                                  <Button
                                    variant="secondary"
                                    size="sm"
                                    className="h-6 text-[11px] px-2 text-danger hover:text-danger"
                                    onClick={() => void handleRevertOutOfScope(task)}
                                  >
                                    {t("pipeline.revertFiles")}
                                  </Button>
                                )}
                                <Button
                                  variant="secondary"
                                  size="sm"
                                  className="h-6 text-[11px] px-2 text-danger hover:text-danger"
                                  onClick={() => openTakeover(task)}
                                >
                                  {t("pipeline.takeover")}
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
                      {renderLogMsg(formatTaskWaitReason(waitReason))}
                    </p>
                  )}

                  <div className="mt-2 flex items-center justify-between text-[11px] text-muted">
                    <div className="flex items-center gap-2">
                      <span>{t("pipeline.depends", { deps: task.depends_on.length > 0 ? task.depends_on.join(", ") : t("pipeline.dependsNone") })}</span>
                      {task.write_scope.length > 0 && (
                        <span>{t("pipeline.scope", { path: task.write_scope[0] })}</span>
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
                          {t("pipeline.startExecution")}
                        </Button>
                      )}
                      {(state === "RUNNING" || state === "VERIFYING") && (
                        <Button
                          variant="secondary"
                          size="sm"
                          className="h-6 text-[11px] px-2 border-danger text-danger hover:bg-danger/10"
                          onClick={() => handleCancelTask(task)}
                        >
                          {t("pipeline.cancelTask")}
                        </Button>
                      )}
                      {state === "INTERRUPTED" && (
                        <Button
                          variant="primary"
                          size="sm"
                          className="h-6 text-[11px] px-2"
                          onClick={() => handleContinueTask(task)}
                        >
                          {t("pipeline.continue")}
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
                            {t("pipeline.rerunFull")}
                          </Button>
                          {(pipeline?.taskFailureEvidence?.[task.id]?.outOfScopeFiles.length ?? 0) > 0 && (
                            <Button
                              variant="secondary"
                              size="sm"
                              className="h-6 text-[11px] px-2 border-danger text-danger hover:bg-danger/10"
                              onClick={() => void handleRevertOutOfScope(task)}
                            >
                              {t("pipeline.revertFiles")}
                            </Button>
                          )}
                          <Button
                            variant="secondary"
                            size="sm"
                            className="h-6 text-[11px] px-2 gap-1 border-danger text-danger hover:bg-danger/10"
                            onClick={() => openTakeover(task)}
                          >
                            <Terminal className="size-3" />
                            {t("pipeline.terminalTakeover")}
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
                  {selectedTask.goal || t("pipeline.noGoal")}
                </p>
              </div>

              {/* Model assignment & Duration */}
              <div className="rounded-md border border-border p-3 bg-subtle/10 space-y-2">
                <span className="text-xs font-semibold text-fg">{t("pipeline.inspectorModel")}</span>
                <div className="space-y-2 text-xs">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-muted">{t("pipeline.assignedModel")}</span>
                    <TaskModelPicker
                      value={selectedTask.assigned_model}
                      placeholder={t("pipeline.pickModel")}
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
                      <span className="text-muted">{t("pipeline.duration")}</span>
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
                <span className="text-xs font-semibold text-fg">{t("pipeline.constraints")}</span>
                <div className="space-y-1.5 text-xs">
                  <div>
                    <span className="text-muted">{t("pipeline.writeScope")}</span>
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
                      <span className="text-muted">{t("pipeline.deliverables")}</span>
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
                    <span className="text-muted">{t("pipeline.upstreamDepends")}</span>
                    <div className="mt-0.5 font-mono text-[11px] text-fg">
                      {selectedTask.depends_on.length > 0 ? selectedTask.depends_on.join(", ") : t("pipeline.noUpstream")}
                    </div>
                  </div>
                </div>
              </div>

              {/* Acceptance Gates & Verification */}
              <div className="rounded-md border border-border p-3 bg-subtle/10 space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-semibold text-fg flex items-center gap-1.5">
                    <CheckCircle2 className="size-3.5 text-accent" />
                    {t("pipeline.gates")}
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
                  <span className="text-[11px] text-subtle">{t("pipeline.noGateCommands")}</span>
                )}

                {pipeline?.gateResults && pipeline.gateResults.filter((g) => g.taskId === selectedTask.id).length > 0 && (
                  <div className="mt-2 space-y-1.5 pt-2 border-t border-border/40">
                    <span className="text-[11px] font-medium text-fg">{t("pipeline.gateHistory")}</span>
                    {pipeline.gateResults
                      .filter((g) => g.taskId === selectedTask.id)
                      .map((gr, idx) => (
                        <div key={idx} className="rounded border border-border/80 bg-bg p-2 text-[11px] font-mono">
                          <div className="flex items-center justify-between">
                            <span className="text-fg font-semibold truncate max-w-[180px]">{gr.command}</span>
                            <span className={gr.passed ? "text-emerald-400" : "text-rose-400"}>
                              {gr.passed ? t("pipeline.gatePassed") : t("pipeline.gateFailed", { code: gr.exitCode ?? "?" })}
                            </span>
                          </div>
                          <div className="mt-0.5 text-muted text-[10px]">
                            {t("pipeline.gateDuration", { ms: gr.durationMs })}
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
                    {t("pipeline.pendingApprovals")}
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
                        {t("pipeline.noCliSession")}
                      </p>
                    )}
                </div>
              )}

              {/* Real-time Sub-agent Execution Logs */}
              {pipeline?.taskLogs?.[selectedTask.id] && pipeline.taskLogs[selectedTask.id].length > 0 && (
                <div className="rounded-md border border-border p-3 bg-subtle/10 space-y-2">
                  <span className="text-xs font-semibold text-fg flex items-center gap-1.5">
                    <Terminal className="size-3.5 text-accent" />
                    {t("pipeline.taskLogs")}
                  </span>
                  <pre
                    data-selectable
                    className="max-h-56 overflow-y-auto rounded bg-bg p-2 text-[11px] font-mono text-muted whitespace-pre-wrap leading-relaxed border border-border/60"
                  >
                    {renderLogMsgs(pipeline.taskLogs[selectedTask.id])}
                  </pre>
                </div>
              )}

            </>
          ) : (
            <div className="flex flex-1 items-center justify-center text-xs text-subtle">
              {t("pipeline.selectTask")}
            </div>
          )}
        </div>
      </div>

      {/* Terminal Takeover Modal (F10.c) */}
      <Modal
        open={takeoverModalOpen}
        onClose={() => setTakeoverModalOpen(false)}
        title={t("pipeline.takeoverModalTitle", { id: takeoverTask?.id ?? "" })}
      >
        <div className="space-y-4 text-xs">
          <p className="text-muted leading-relaxed">
            {t("pipeline.takeoverModalBody")}
          </p>
          {resumeCmd ? (
            <div className="bg-bg p-3 rounded-md font-mono text-[12px] border border-border text-fg select-all">
              {resumeCmd}
            </div>
          ) : (
            <div className="bg-bg p-3 rounded-md text-[12px] border border-border text-warning">
              {(resumeHint && renderLogMsg(resumeHint)) || t("pipeline.noResumeSession")}
            </div>
          )}
          <p className="text-[11px] text-muted">
            {t("pipeline.takeoverModalHint")}
          </p>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={() => setTakeoverModalOpen(false)}>
              {t("pipeline.later")}
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
              {t("pipeline.respawnAgent")}
            </Button>
            <Button
              variant="primary"
              disabled={takeoverRerunning}
              onClick={handleTakeoverRerunGates}
            >
              {takeoverRerunning ? t("pipeline.takeoverRerunning") : t("pipeline.takeoverRerunGates")}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}

function TaskStateBadge({ state }: { state: TaskState }) {
  const { t } = useTranslation();
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
          {t("pipeline.statePausedQuota")}
        </Badge>
      );
    case "WAITING_UPSTREAM":
      return (
        <Badge variant="muted" className="gap-1 text-[10px]">
          <Clock className="size-3 text-muted" />
          {t("pipeline.stateWaitingUpstream")}
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
