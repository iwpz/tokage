import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import i18n from "../../i18n";
import {
  AlertCircle,
  AlertTriangle,
  CheckCircle2,
  Code2,
  Compass,
  FileText,
  ListTodo,
  MessageSquare,
  Play,
  RefreshCw,
  RotateCcw,
  Save,
  Wand2,
  XCircle,
} from "lucide-react";
import { usePlanSession } from "./usePlanSession";
import { SpecEditor } from "./SpecEditor";
import { ContractsEditor } from "./ContractsEditor";
import { TasksTable } from "./TasksTable";
import { Badge, Button, Modal } from "../ui";
import { cn } from "../../lib/cn";

import {
  allocatePlanModels,
  deriveSessionTitleFromSpec,
  persistPlan,
  recordPlanApproved,
} from "../../lib/ipc";
import { fixedModelOverrides } from "../../lib/fixedModel";
import { useChat } from "../../stores/chat";
import { useExecution } from "../../stores/execution";
import { useProjects } from "../../stores/projects";
import { useSettings } from "../../stores/settings";
import { dispatchReadyTasks, syncMainSessionState } from "../../lib/taskRunner";
import { localizeIssue } from "../../lib/planIssue";

export type PlanSubTab = "spec" | "contracts" | "tasks";

export function PlanReview({
  sessionId,
  onSwitchTab,
  onRequestReplan,
  onApprove,
}: {
  sessionId: string;
  onSwitchTab?: (tab: any) => void;
  onRequestReplan?: (feedback: string) => void;
  onApprove?: () => void;
}) {
  const { t } = useTranslation();
  const {
    plan,
    setPlan,
    allocation,
    runAllocation,
    loading,
    saving,
    repairing,
    dirty,
    error,
    validation,
    updateSpec,
    updateContract,
    addContract,
    removeContract,
    updateTask,
    addTask,
    removeTask,
    save,
    saveUserEdited,
    resetToDefault,
    reload,
    repairWithModel,
    cwd,
  } = usePlanSession(sessionId);

  const startPlanExecution = useExecution((s) => s.startPlanExecution);
  const requireReview = useSettings((s) => s.requireReview);
  const autoApproveBelow = useSettings((s) => s.planningAutoApproveBelowUsd);
  const modelLocked = useChat((s) => !!s.conversations[sessionId]?.lockedModel);

  // All useState/useRef must stay above any early return (loading / empty plan).
  const [subTab, setSubTab] = useState<PlanSubTab>("spec");
  const autoApproveFired = useRef(false);
  const [autoApproveHint, setAutoApproveHint] = useState<string | null>(null);
  const [startConfirmModalOpen, setStartConfirmModalOpen] = useState(false);
  const [discardModalOpen, setDiscardModalOpen] = useState(false);
  const [feedbackModalOpen, setFeedbackModalOpen] = useState(false);
  const [feedbackText, setFeedbackText] = useState("");
  const [approving, setApproving] = useState(false);
  const [approveSuccess, setApproveSuccess] = useState(false);

  // F13.b: plan awaiting confirmation → review
  useEffect(() => {
    if (!plan?.tasks?.length) return;
    const project = useProjects.getState().projects.find((p) =>
      p.sessions.some((s) => s.id === sessionId),
    );
    const session = project?.sessions.find((s) => s.id === sessionId);
    if (session?.state === "draft") {
      useProjects.getState().setSessionState(sessionId, "review");
    }
  }, [sessionId, plan?.tasks?.length]);

  const issues = validation?.issues ?? [];
  const errorCount = issues.filter((i) => i.severity === "error").length;
  const warningCount = issues.filter((i) => i.severity === "warning").length;

  const handleApprove = async () => {
    if (errorCount > 0) return;
    setApproving(true);
    try {
      // Use the validated/repaired plan from save(), not the possibly-stale closure `plan` (F1.e).
      const saved = await save();
      setApproveSuccess(true);
      if (saved?.tasks && saved.tasks.length > 0 && cwd) {
        // Re-allocate at confirm time; manual picks already on task.assigned_model are honored as overrides (F1.f).
        // Single-model mode wins over per-task picks so the whole plan stays on one model.
        const locked = useChat.getState().conversations[sessionId]?.lockedModel;
        let overrides: Record<string, string>;
        if (locked) {
          overrides = fixedModelOverrides(saved, locked);
        } else {
          overrides = {};
          for (const t of saved.tasks) {
            if (t.assigned_model) overrides[t.id] = t.assigned_model;
          }
        }
        const alloc = await allocatePlanModels(saved, overrides);
        const tasksWithModels = saved.tasks.map((t) => {
          const a = alloc.assignments.find((x) => x.taskId === t.id);
          if (!a) return t;
          return {
            ...t,
            assigned_model: `${a.provider}:${a.cliModelArg}`,
          };
        });
        const planToRun = { ...saved, tasks: tasksWithModels };
        await persistPlan(cwd, sessionId, planToRun);
        setPlan(planToRun);

        startPlanExecution(
          sessionId,
          planToRun.tasks,
          cwd,
          planToRun.contracts ?? [],
          alloc.assignments,
        );

        // F13.b: session state + title from spec when still default
        useProjects.getState().setSessionState(sessionId, "running");
        try {
          const project = useProjects.getState().projects.find((p) =>
            p.sessions.some((s) => s.id === sessionId),
          );
          const session = project?.sessions.find((s) => s.id === sessionId);
          const defaultTitles = new Set(["新会话", "New Session", ""]);
          if (session && defaultTitles.has(session.title.trim())) {
            const title = await deriveSessionTitleFromSpec(planToRun.spec);
            if (title.trim()) useProjects.getState().renameSession(sessionId, title.trim());
          }
        } catch (err) {
          console.warn("[PlanReview] derive_session_title_from_spec failed:", err);
        }

        // F12.c: persist relational plan rows; store planDbId / taskDbIds for usage attribution.
        try {
          const recorded = await recordPlanApproved({
            sessionId,
            plan: planToRun,
            assignments: alloc.assignments,
            estCostUsd: alloc.totalEstCostUsd ?? 0,
          });
          useExecution.getState().setPlanDbIds(sessionId, recorded.planId, recorded.taskIds);
        } catch (err) {
          console.warn("[PlanReview] record_plan_approved failed:", err);
        }

        const startingTasks = planToRun.tasks.filter((t) => t.depends_on.length === 0);
        const taskNames = startingTasks
          .map((task) => `**${task.id} (${task.title})**`)
          .join(i18n.language === "zh" ? "、" : ", ");
        useChat.getState().postSystemMessage(
          sessionId,
          t("plan.review.approvedSystemMessage", { taskNames }),
        );
        void dispatchReadyTasks(sessionId);
        syncMainSessionState(sessionId);
      }
      setStartConfirmModalOpen(false);
      setTimeout(() => {
        onApprove?.();
        onSwitchTab?.("chat");
      }, 500);
    } finally {
      setApproving(false);
    }
  };

  // F16.f: auto-approve when review not required, or cost below threshold.
  // Must stay above early returns so hook count is stable across loading/empty states.
  useEffect(() => {
    if (autoApproveFired.current || approving || approveSuccess) return;
    if (!plan?.tasks?.length) return;
    const errors = (validation?.issues ?? []).filter((i) => i.severity === "error");
    if (errors.length > 0) return;
    if (validation && !validation.isValid) return;

    const est = allocation?.totalEstCostUsd ?? 0;
    const byReviewOff = !requireReview;
    const byBudget = autoApproveBelow > 0 && est > 0 && est < autoApproveBelow;
    if (!byReviewOff && !byBudget) return;

    autoApproveFired.current = true;
    const reason = byReviewOff
      ? t("plan.review.autoApproveReviewOff")
      : t("plan.review.autoApproveBudget", {
          est: est.toFixed(2),
          threshold: autoApproveBelow.toFixed(2),
        });
    setAutoApproveHint(reason);
    const timer = window.setTimeout(() => {
      void handleApprove();
    }, 1200);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- fire once when plan becomes valid
  }, [
    plan?.tasks?.length,
    validation?.isValid,
    allocation?.totalEstCostUsd,
    requireReview,
    autoApproveBelow,
    approving,
    approveSuccess,
  ]);

  if (loading) {
    return (
      <div className="flex flex-1 items-center justify-center p-8 text-xs text-muted gap-2">
        <RefreshCw className="size-4 animate-spin text-accent" />
        <span>{t("plan.review.loading")}</span>
      </div>
    );
  }

  const isPlanEmpty =
    !plan ||
    (!plan.spec.goal &&
      plan.tasks.length === 0 &&
      plan.contracts.length === 0 &&
      (!plan.spec.raw || !plan.spec.raw.trim()) &&
      (!plan.rawMarkdown || !plan.rawMarkdown.trim()));

  if (isPlanEmpty) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
        <FileText className="size-10 text-subtle mb-3" />
        <h3 className="text-sm font-medium text-fg">{t("plan.review.emptyTitle")}</h3>
        <p className="mt-1 text-xs text-muted max-w-sm">
          {t("plan.review.emptyBody")}
        </p>
        <div className="mt-4 flex items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            onClick={() => void reload()}
            className="gap-1.5 text-xs"
          >
            <RefreshCw className="size-3.5" />
            <span>{t("plan.review.reload")}</span>
          </Button>
          <Button
            size="sm"
            variant="primary"
            onClick={() => void repairWithModel()}
            disabled={repairing}
            className="gap-1.5 text-xs"
          >
            <Wand2 className="size-3.5" />
            <span>{repairing ? t("plan.review.repairing") : t("plan.review.repairFormat")}</span>
          </Button>
        </div>
      </div>
    );
  }

  const handleConfirmDiscard = () => {
    setDiscardModalOpen(false);
    setPlan({
      spec: { raw: "", goal: "", inScope: [], outOfScope: [], decisions: [] },
      contracts: [],
      tasks: [],
      notes: "",
      rawMarkdown: "",
    });
    onSwitchTab?.("chat");
  };

  const handleConfirmFeedback = () => {
    if (!feedbackText.trim()) return;
    const text = feedbackText.trim();
    setFeedbackModalOpen(false);
    setFeedbackText("");
    onRequestReplan?.(text);
    onSwitchTab?.("chat");
  };

  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-base">
      {autoApproveHint && (
        <div className="border-b border-accent/30 bg-accent/10 px-5 py-2 text-xs text-accent">
          {autoApproveHint}{t("plan.review.autoApprovePauseSuffix")}
        </div>
      )}
      {/* Top action & status bar */}
      <div className="flex items-center justify-between border-b border-border bg-elevated/60 px-5 py-2">
        <div className="flex items-center gap-2">
          {plan.version && (
            <Badge variant="muted" size="xs" className="font-mono text-[11px] font-semibold text-accent">
              v{plan.version}
            </Badge>
          )}
          {plan.parentPlanId && (
            <Badge variant="muted" size="xs" className="font-mono text-[10px] text-muted">
              {t("plan.review.basedOn", { parentPlanId: plan.parentPlanId })}
            </Badge>
          )}
          <div role="tablist" aria-label={t("plan.review.tabsAria")} className="flex items-center gap-1">
            <button
              type="button"
              role="tab"
              aria-selected={subTab === "spec"}
              onClick={() => setSubTab("spec")}
              className={cn(
                "inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium transition-colors",
                subTab === "spec" ? "bg-active text-fg shadow-xs" : "text-muted hover:bg-hover hover:text-fg",
              )}
            >
              <Compass className="size-3.5" />
              <span>{t("plan.review.tabSpec")}</span>
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={subTab === "contracts"}
              onClick={() => setSubTab("contracts")}
              className={cn(
                "inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium transition-colors",
                subTab === "contracts" ? "bg-active text-fg shadow-xs" : "text-muted hover:bg-hover hover:text-fg",
              )}
            >
              <Code2 className="size-3.5" />
              <span>{t("plan.review.tabContracts")}</span>
              {plan.contracts.length > 0 && (
                <span className="rounded bg-hover px-1.5 py-0.2 text-[10px] tabular-nums font-mono">
                  {plan.contracts.length}
                </span>
              )}
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={subTab === "tasks"}
              onClick={() => setSubTab("tasks")}
              className={cn(
                "inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium transition-colors",
                subTab === "tasks" ? "bg-active text-fg shadow-xs" : "text-muted hover:bg-hover hover:text-fg",
              )}
            >
              <ListTodo className="size-3.5" />
              <span>{t("plan.review.tabTasks")}</span>
              {plan.tasks.length > 0 && (
                <span className="rounded bg-hover px-1.5 py-0.2 text-[10px] tabular-nums font-mono">
                  {plan.tasks.length}
                </span>
              )}
            </button>
          </div>
        </div>

        {/* Right action group */}
        <div className="flex items-center gap-2">
          {plan.isUserEdited && (
            <span title={t("plan.review.userEditedTitle")}>
              <Badge variant="accent" size="xs">
                {t("plan.review.userEdited")}
              </Badge>
            </span>
          )}

          {dirty ? (
            <Badge variant="warning" size="xs">
              {t("plan.review.unsaved")}
            </Badge>
          ) : validation?.isValid ? (
            <Badge variant="success" size="xs">
              <CheckCircle2 className="size-3" />
              {t("plan.review.valid")}
            </Badge>
          ) : null}

          {plan.isUserEdited && (
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void resetToDefault()}
              title={t("plan.review.resetDefaultTitle")}
              className="h-7 gap-1 px-2 text-xs text-muted hover:text-danger hover:border-danger/30"
            >
              <RotateCcw className="size-3" />
              <span>{t("plan.review.resetDefault")}</span>
            </Button>
          )}

          <Button
            size="sm"
            variant="secondary"
            onClick={() => void reload()}
            title={t("plan.review.reloadFromDisk")}
            className="h-7 px-2 text-xs text-muted hover:text-fg"
          >
            <RefreshCw className="size-3" />
          </Button>

          <Button
            size="sm"
            variant="primary"
            onClick={() => void save()}
            disabled={saving || !dirty}
            className="h-7 gap-1 text-xs"
          >
            <Save className="size-3" />
            <span>{saving ? t("plan.review.saving") : t("plan.review.save")}</span>
          </Button>
        </div>
      </div>

      {/* Validation or Error notification banner */}
      {error && (
        <div className="flex items-center gap-2 border-b border-danger/30 bg-danger/10 px-5 py-2 text-xs text-danger">
          <AlertTriangle className="size-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {validation && !validation.isValid && (
        <div className="flex flex-col gap-1 border-b border-warning/30 bg-warning/10 px-5 py-2 text-xs text-warning">
          <div className="flex items-center gap-1.5 font-semibold">
            <AlertTriangle className="size-3.5" />
            <span>{t("plan.review.validationIssues", { count: validation.issues.length })}</span>
          </div>
          <ul className="list-disc pl-5 space-y-0.5">
            {validation.issues.map((issue, idx) => (
              <li key={idx}>
                {issue.taskId ? `[${issue.taskId}] ` : ""}
                {localizeIssue(issue)}
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Main Tab Panels */}
      <div className="flex-1 overflow-hidden">
        {subTab === "spec" && (
          <SpecEditor
            spec={plan.spec}
            onChange={(patch) => {
              updateSpec(patch);
              const updatedPlan = {
                ...plan,
                spec: { ...plan.spec, ...patch },
              };
              void saveUserEdited(updatedPlan);
            }}
          />
        )}
        {subTab === "contracts" && (
          <ContractsEditor
            contracts={plan.contracts}
            onUpdate={(id, patch) => {
              updateContract(id, patch);
              const updatedPlan = {
                ...plan,
                contracts: plan.contracts.map((c) => (c.id === id ? { ...c, ...patch } : c)),
              };
              void saveUserEdited(updatedPlan);
            }}
            onAdd={(c) => {
              addContract(c);
              const updatedPlan = {
                ...plan,
                contracts: [...plan.contracts, c],
              };
              void saveUserEdited(updatedPlan);
            }}
            onRemove={(id) => {
              removeContract(id);
              const updatedPlan = {
                ...plan,
                contracts: plan.contracts.filter((c) => c.id !== id),
              };
              void saveUserEdited(updatedPlan);
            }}
          />
        )}
        {subTab === "tasks" && (
          <TasksTable
            tasks={plan.tasks}
            assignments={allocation?.assignments}
            validationIssues={validation?.issues}
            modelLocked={modelLocked}
            onUpdateTask={(id, patch) => {
              updateTask(id, patch);
              const updatedPlan = {
                ...plan,
                tasks: plan.tasks.map((t) => (t.id === id ? { ...t, ...patch } : t)),
              };
              void saveUserEdited(updatedPlan);
            }}
            onAddTask={(t) => {
              addTask(t);
              const updatedPlan = {
                ...plan,
                tasks: [...plan.tasks, t],
              };
              void saveUserEdited(updatedPlan);
            }}
            onRemoveTask={(id) => {
              removeTask(id);
              const updatedPlan = {
                ...plan,
                tasks: plan.tasks.filter((t) => t.id !== id),
              };
              void saveUserEdited(updatedPlan);
            }}
            onReassign={(taskId, modelSpec) => {
              const updatedPlan = {
                ...plan,
                tasks: plan.tasks.map((t) =>
                  t.id === taskId ? { ...t, assigned_model: modelSpec || null } : t
                ),
              };
              updateTask(taskId, { assigned_model: modelSpec || null });
              void saveUserEdited(updatedPlan);
              void runAllocation({ [taskId]: modelSpec });
            }}
          />
        )}
      </div>

      {/* Bottom Floating Actions & Review Control Bar */}
      <div className="mx-4 mt-2.5 mb-4 flex shrink-0 items-center justify-between rounded-xl border border-border bg-elevated px-5 py-2.5 shadow-sm">
        {/* Left summary & risk badges */}
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-1.5 text-xs text-muted">
            <span className="font-semibold text-fg">{t("plan.review.summaryTasks", { count: plan.tasks.length })}</span>
            <span>·</span>
            <span className="font-semibold text-fg">{t("plan.review.summaryContracts", { count: plan.contracts.length })}</span>
            {allocation && (
              <>
                <span>·</span>
                <span>{t("plan.review.estTotalCost")} </span>
                <span className="font-semibold text-accent">${allocation.totalEstCostUsd.toFixed(2)}</span>
              </>
            )}
          </div>

          <div className="flex items-center gap-1.5">
            {errorCount > 0 ? (
              <Badge variant="danger" size="xs" className="gap-1">
                <AlertCircle className="size-3" />
                <span>{t("plan.review.blockingErrors", { count: errorCount })}</span>
              </Badge>
            ) : warningCount > 0 ? (
              <Badge variant="warning" size="xs" className="gap-1">
                <AlertTriangle className="size-3" />
                <span>{t("plan.review.warnings", { count: warningCount })}</span>
              </Badge>
            ) : validation?.isValid ? (
              <Badge variant="success" size="xs" className="gap-1">
                <CheckCircle2 className="size-3" />
                <span>{t("plan.review.allValid")}</span>
              </Badge>
            ) : null}
          </div>
        </div>

        {/* Right review actions: Discard, Feedback, Approve */}
        <div className="flex items-center gap-2.5">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setDiscardModalOpen(true)}
            className="h-8 gap-1.5 text-xs text-muted hover:bg-danger/10 hover:text-danger"
          >
            <XCircle className="size-3.5" />
            <span>{t("plan.review.discard")}</span>
          </Button>

          <Button
            size="sm"
            variant="secondary"
            onClick={() => setFeedbackModalOpen(true)}
            className="h-8 gap-1.5 text-xs"
          >
            <MessageSquare className="size-3.5" />
            <span>{t("plan.review.feedback")}</span>
          </Button>

          <Button
            size="sm"
            variant="primary"
            onClick={() => setStartConfirmModalOpen(true)}
            disabled={approving || errorCount > 0}
            className="h-8 gap-1.5 text-xs font-semibold px-4"
          >
            {approveSuccess ? (
              <>
                <CheckCircle2 className="size-3.5 text-emerald-400" />
                <span>{t("plan.review.approvedEntering")}</span>
              </>
            ) : (
              <>
                <Play className="size-3.5" />
                <span>{approving ? t("plan.review.approving") : t("plan.review.confirmExecute")}</span>
              </>
            )}
          </Button>
        </div>
      </div>

      {/* Start Execution Double-Confirmation Modal */}
      <Modal
        open={startConfirmModalOpen}
        onClose={() => setStartConfirmModalOpen(false)}
        title={t("plan.review.confirmModal.title")}
      >
        <div className="flex flex-col gap-4 text-xs">
          <p className="leading-relaxed text-muted">
            {t("plan.review.confirmModal.body")}
          </p>

          <div className="rounded-lg border border-border bg-base p-3 space-y-2.5">
            <div className="flex items-center justify-between text-fg font-medium">
              <span>{t("plan.review.confirmModal.taskTotal", { count: plan?.tasks.length ?? 0 })}</span>
              {allocation && (
                <span className="text-subtle font-mono text-[11px]">
                  {t("plan.review.confirmModal.estCost", { cost: allocation.totalEstCostUsd.toFixed(2) })}
                </span>
              )}
            </div>

            {/* Parallel Starters list */}
            <div>
              <div className="text-[11px] font-semibold text-accent mb-1 flex items-center gap-1">
                <span>{t("plan.review.confirmModal.parallelHeader")}</span>
                <span>
                  ({plan?.tasks.filter((t) => t.depends_on.length === 0).length ?? 0})
                </span>
              </div>
              <div className="space-y-1 max-h-36 overflow-y-auto pr-1">
                {plan?.tasks
                  .filter((task) => task.depends_on.length === 0)
                  .map((task) => (
                    <div
                      key={task.id}
                      className="flex items-center justify-between rounded bg-elevated px-2 py-1 text-[11px]"
                    >
                      <div className="flex items-center gap-1.5 truncate">
                        <span className="font-mono font-bold text-accent shrink-0">{task.id}</span>
                        <span className="truncate text-fg">{task.title}</span>
                      </div>
                      <span className="text-subtle text-[10px] shrink-0 font-mono">
                        {task.domain}
                      </span>
                    </div>
                  ))}
              </div>
            </div>

            {/* Pending Tasks list */}
            {plan?.tasks.some((t) => t.depends_on.length > 0) && (
              <div>
                <div className="text-[11px] font-medium text-muted mb-1 flex items-center gap-1">
                  <span>{t("plan.review.confirmModal.queuedHeader")}</span>
                  <span>
                    ({plan?.tasks.filter((t) => t.depends_on.length > 0).length ?? 0})
                  </span>
                </div>
                <div className="space-y-1 max-h-28 overflow-y-auto pr-1">
                  {plan?.tasks
                    .filter((task) => task.depends_on.length > 0)
                    .map((task) => (
                      <div
                        key={task.id}
                        className="flex items-center justify-between rounded bg-elevated/60 px-2 py-1 text-[11px] text-muted"
                      >
                        <div className="flex items-center gap-1.5 truncate">
                          <span className="font-mono text-muted shrink-0">{task.id}</span>
                          <span className="truncate">{task.title}</span>
                        </div>
                        <span className="text-subtle text-[10px] shrink-0 font-mono">
                          {t("plan.review.confirmModal.needsDeps", { deps: task.depends_on.join(", ") })}
                        </span>
                      </div>
                    ))}
                </div>
              </div>
            )}
          </div>

          <div className="flex items-center justify-end gap-2 pt-2 border-t border-border">
            <Button size="sm" variant="ghost" onClick={() => setStartConfirmModalOpen(false)}>
              {t("plan.review.confirmModal.back")}
            </Button>
            <Button
              size="sm"
              variant="primary"
              onClick={() => void handleApprove()}
              disabled={approving}
              className="gap-1.5"
            >
              <Play className="size-3.5" />
              <span>{approving ? t("plan.review.confirmModal.starting") : t("plan.review.confirmModal.confirmStart")}</span>
            </Button>
          </div>
        </div>
      </Modal>

      {/* Discard Confirmation Modal */}
      <Modal
        open={discardModalOpen}
        onClose={() => setDiscardModalOpen(false)}
        title={t("plan.review.discardModal.title")}
      >
        <div className="flex flex-col gap-4 text-xs">
          <p className="leading-relaxed text-muted">
            {t("plan.review.discardModal.body")}
          </p>
          <div className="flex items-center justify-end gap-2 pt-2 border-t border-border">
            <Button size="sm" variant="ghost" onClick={() => setDiscardModalOpen(false)}>
              {t("plan.review.discardModal.keep")}
            </Button>
            <Button size="sm" variant="primary" onClick={handleConfirmDiscard} className="bg-danger text-white">
              {t("plan.review.discardModal.confirm")}
            </Button>
          </div>
        </div>
      </Modal>

      {/* Feedback / Re-plan Input Modal */}
      <Modal
        open={feedbackModalOpen}
        onClose={() => setFeedbackModalOpen(false)}
        title={t("plan.review.feedbackModal.title")}
      >
        <div className="flex flex-col gap-3 text-xs">
          <p className="leading-relaxed text-muted">
            {t("plan.review.feedbackModal.body")}
          </p>
          <textarea
            rows={5}
            value={feedbackText}
            onChange={(e) => setFeedbackText(e.target.value)}
            placeholder={t("plan.review.feedbackModal.placeholder")}
            className="w-full rounded-lg border border-border bg-base p-3 text-xs text-fg placeholder:text-subtle focus:border-accent focus:outline-hidden"
          />
          <div className="flex items-center justify-end gap-2 pt-2 border-t border-border">
            <Button size="sm" variant="ghost" onClick={() => setFeedbackModalOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              size="sm"
              variant="primary"
              disabled={!feedbackText.trim()}
              onClick={handleConfirmFeedback}
            >
              {t("plan.review.feedbackModal.submit")}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
