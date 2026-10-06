import { useState } from "react";
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

import { useChat } from "../../stores/chat";
import { useExecution } from "../../stores/execution";
import { dispatchReadyTasks } from "../../lib/taskRunner";

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
  const [subTab, setSubTab] = useState<PlanSubTab>("spec");

  // Action Modals State
  const [startConfirmModalOpen, setStartConfirmModalOpen] = useState(false);
  const [discardModalOpen, setDiscardModalOpen] = useState(false);
  const [feedbackModalOpen, setFeedbackModalOpen] = useState(false);
  const [feedbackText, setFeedbackText] = useState("");
  const [approving, setApproving] = useState(false);
  const [approveSuccess, setApproveSuccess] = useState(false);

  if (loading) {
    return (
      <div className="flex flex-1 items-center justify-center p-8 text-xs text-muted gap-2">
        <RefreshCw className="size-4 animate-spin text-accent" />
        <span>正在加载计划规约...</span>
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
        <h3 className="text-sm font-medium text-fg">未检测到计划规约</h3>
        <p className="mt-1 text-xs text-muted max-w-sm">
          当前会话尚未生成标准技术计划。可在对话中以「先规划」模式发送需求，或在下方点击调用便宜模型对已有回复进行规约格式修复。
        </p>
        <div className="mt-4 flex items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            onClick={() => void reload()}
            className="gap-1.5 text-xs"
          >
            <RefreshCw className="size-3.5" />
            <span>重新载入</span>
          </Button>
          <Button
            size="sm"
            variant="primary"
            onClick={() => void repairWithModel()}
            disabled={repairing}
            className="gap-1.5 text-xs"
          >
            <Wand2 className="size-3.5" />
            <span>{repairing ? "模型修复格式中..." : "调用便宜模型修复格式"}</span>
          </Button>
        </div>
      </div>
    );
  }

  const issues = validation?.issues ?? [];
  const errorCount = issues.filter((i) => i.severity === "error").length;
  const warningCount = issues.filter((i) => i.severity === "warning").length;

  const handleApprove = async () => {
    if (errorCount > 0) return;
    setApproving(true);
    try {
      await save();
      setApproveSuccess(true);
      if (plan?.tasks && plan.tasks.length > 0) {
        startPlanExecution(sessionId, plan.tasks, cwd ?? undefined, plan.contracts ?? []);
        const startingTasks = plan.tasks.filter((t) => t.depends_on.length === 0);
        const taskNames = startingTasks.map((t) => `**${t.id} (${t.title})**`).join("、");
        useChat.getState().postSystemMessage(
          sessionId,
          `🚀 **技术计划已确认，子代理流水线正式启动！**\n\n首批并行执行任务：${taskNames}\n\n执行看板已就绪，正在分配指定模型进行代码编写与本地自动化校验。可在下方查看各子代理实时作业状态。`
        );
        // Kick off real sub-agent dispatch
        void dispatchReadyTasks(sessionId);
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
              基于: {plan.parentPlanId}
            </Badge>
          )}
          <div role="tablist" aria-label="计划视图子标签" className="flex items-center gap-1">
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
              <span>设计稿 (Spec)</span>
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
              <span>契约 (Contracts)</span>
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
              <span>任务图表 (Tasks)</span>
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
            <span title="当前加载的是用户编辑版本 (user_edited)">
              <Badge variant="accent" size="xs">
                用户已编辑
              </Badge>
            </span>
          )}

          {dirty ? (
            <Badge variant="warning" size="xs">
              未保存修改
            </Badge>
          ) : validation?.isValid ? (
            <Badge variant="success" size="xs">
              <CheckCircle2 className="size-3" />
              校验通过
            </Badge>
          ) : null}

          {plan.isUserEdited && (
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void resetToDefault()}
              title="重新加载初始版本并删除 user_edited 版本"
              className="h-7 gap-1 px-2 text-xs text-muted hover:text-danger hover:border-danger/30"
            >
              <RotateCcw className="size-3" />
              <span>恢复默认</span>
            </Button>
          )}

          <Button
            size="sm"
            variant="secondary"
            onClick={() => void reload()}
            title="重新从磁盘读取"
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
            <span>{saving ? "保存中..." : "保存规约"}</span>
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
            <span>计划校验发现 {validation.issues.length} 处问题：</span>
          </div>
          <ul className="list-disc pl-5 space-y-0.5">
            {validation.issues.map((issue, idx) => (
              <li key={idx}>
                {issue.taskId ? `[${issue.taskId}] ` : ""}
                {issue.message}
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
            <span className="font-semibold text-fg">{plan.tasks.length}</span> 项任务
            <span>·</span>
            <span className="font-semibold text-fg">{plan.contracts.length}</span> 项契约
            {allocation && (
              <>
                <span>·</span>
                <span>预估总成本: </span>
                <span className="font-semibold text-accent">${allocation.totalEstCostUsd.toFixed(2)}</span>
              </>
            )}
          </div>

          <div className="flex items-center gap-1.5">
            {errorCount > 0 ? (
              <Badge variant="danger" size="xs" className="gap-1">
                <AlertCircle className="size-3" />
                <span>{errorCount} 项阻断错误</span>
              </Badge>
            ) : warningCount > 0 ? (
              <Badge variant="warning" size="xs" className="gap-1">
                <AlertTriangle className="size-3" />
                <span>{warningCount} 项潜在风险</span>
              </Badge>
            ) : validation?.isValid ? (
              <Badge variant="success" size="xs" className="gap-1">
                <CheckCircle2 className="size-3" />
                <span>校验全部通过</span>
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
            <span>放弃计划</span>
          </Button>

          <Button
            size="sm"
            variant="secondary"
            onClick={() => setFeedbackModalOpen(true)}
            className="h-8 gap-1.5 text-xs"
          >
            <MessageSquare className="size-3.5" />
            <span>提出修改意见</span>
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
                <span>已确认，进入执行...</span>
              </>
            ) : (
              <>
                <Play className="size-3.5" />
                <span>{approving ? "确认中..." : "确认并开始执行"}</span>
              </>
            )}
          </Button>
        </div>
      </div>

      {/* Start Execution Double-Confirmation Modal */}
      <Modal
        open={startConfirmModalOpen}
        onClose={() => setStartConfirmModalOpen(false)}
        title="确认开始执行技术计划"
      >
        <div className="flex flex-col gap-4 text-xs">
          <p className="leading-relaxed text-muted">
            即将启动子代理并行流水线。系统将为各个任务指派模型并在隔离环境中执行实现与自动化验收关卡：
          </p>

          <div className="rounded-lg border border-border bg-base p-3 space-y-2.5">
            <div className="flex items-center justify-between text-fg font-medium">
              <span>任务总数: {plan?.tasks.length ?? 0} 个</span>
              {allocation && (
                <span className="text-subtle font-mono text-[11px]">
                  预估费用: ${allocation.totalEstCostUsd.toFixed(2)}
                </span>
              )}
            </div>

            {/* Parallel Starters list */}
            <div>
              <div className="text-[11px] font-semibold text-accent mb-1 flex items-center gap-1">
                <span>⚡ 首批可立即并行执行任务:</span>
                <span>
                  ({plan?.tasks.filter((t) => t.depends_on.length === 0).length ?? 0})
                </span>
              </div>
              <div className="space-y-1 max-h-36 overflow-y-auto pr-1">
                {plan?.tasks
                  .filter((t) => t.depends_on.length === 0)
                  .map((t) => (
                    <div
                      key={t.id}
                      className="flex items-center justify-between rounded bg-elevated px-2 py-1 text-[11px]"
                    >
                      <div className="flex items-center gap-1.5 truncate">
                        <span className="font-mono font-bold text-accent shrink-0">{t.id}</span>
                        <span className="truncate text-fg">{t.title}</span>
                      </div>
                      <span className="text-subtle text-[10px] shrink-0 font-mono">
                        {t.domain}
                      </span>
                    </div>
                  ))}
              </div>
            </div>

            {/* Pending Tasks list */}
            {plan?.tasks.some((t) => t.depends_on.length > 0) && (
              <div>
                <div className="text-[11px] font-medium text-muted mb-1 flex items-center gap-1">
                  <span>⏳ 依赖就绪后排队执行任务:</span>
                  <span>
                    ({plan?.tasks.filter((t) => t.depends_on.length > 0).length ?? 0})
                  </span>
                </div>
                <div className="space-y-1 max-h-28 overflow-y-auto pr-1">
                  {plan?.tasks
                    .filter((t) => t.depends_on.length > 0)
                    .map((t) => (
                      <div
                        key={t.id}
                        className="flex items-center justify-between rounded bg-elevated/60 px-2 py-1 text-[11px] text-muted"
                      >
                        <div className="flex items-center gap-1.5 truncate">
                          <span className="font-mono text-muted shrink-0">{t.id}</span>
                          <span className="truncate">{t.title}</span>
                        </div>
                        <span className="text-subtle text-[10px] shrink-0 font-mono">
                          需依赖: {t.depends_on.join(", ")}
                        </span>
                      </div>
                    ))}
                </div>
              </div>
            )}
          </div>

          <div className="flex items-center justify-end gap-2 pt-2 border-t border-border">
            <Button size="sm" variant="ghost" onClick={() => setStartConfirmModalOpen(false)}>
              返回检查
            </Button>
            <Button
              size="sm"
              variant="primary"
              onClick={() => void handleApprove()}
              disabled={approving}
              className="gap-1.5"
            >
              <Play className="size-3.5" />
              <span>{approving ? "启动中..." : "确认并立即启动"}</span>
            </Button>
          </div>
        </div>
      </Modal>

      {/* Discard Confirmation Modal */}
      <Modal
        open={discardModalOpen}
        onClose={() => setDiscardModalOpen(false)}
        title="放弃当前技术计划"
      >
        <div className="flex flex-col gap-4 text-xs">
          <p className="leading-relaxed text-muted">
            是否确定放弃当前技术计划？放弃后当前草稿中的所有设计稿（Spec）、契约（Contracts）与任务图表（Tasks）将被重置，并返回直接对话。
          </p>
          <div className="flex items-center justify-end gap-2 pt-2 border-t border-border">
            <Button size="sm" variant="ghost" onClick={() => setDiscardModalOpen(false)}>
              保留计划
            </Button>
            <Button size="sm" variant="primary" onClick={handleConfirmDiscard} className="bg-danger text-white">
              确认放弃
            </Button>
          </div>
        </div>
      </Modal>

      {/* Feedback / Re-plan Input Modal */}
      <Modal
        open={feedbackModalOpen}
        onClose={() => setFeedbackModalOpen(false)}
        title="提出修改意见 (触发重新规划)"
      >
        <div className="flex flex-col gap-3 text-xs">
          <p className="leading-relaxed text-muted">
            描述你对设计稿、契约或任务划分的调整要求。Tokage 将保留已有上下文并针对你的意见进行增量重规划：
          </p>
          <textarea
            rows={5}
            value={feedbackText}
            onChange={(e) => setFeedbackText(e.target.value)}
            placeholder="例如：请把 T2 中的前端页面拆分为列表页和详情页两个任务，并补充相关的单元测试标准..."
            className="w-full rounded-lg border border-border bg-base p-3 text-xs text-fg placeholder:text-subtle focus:border-accent focus:outline-hidden"
          />
          <div className="flex items-center justify-end gap-2 pt-2 border-t border-border">
            <Button size="sm" variant="ghost" onClick={() => setFeedbackModalOpen(false)}>
              取消
            </Button>
            <Button
              size="sm"
              variant="primary"
              disabled={!feedbackText.trim()}
              onClick={handleConfirmFeedback}
            >
              发送意见并重排
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
