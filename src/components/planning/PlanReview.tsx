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
  Save,
  XCircle,
} from "lucide-react";
import { usePlanSession } from "./usePlanSession";
import { SpecEditor } from "./SpecEditor";
import { ContractsEditor } from "./ContractsEditor";
import { TasksTable } from "./TasksTable";
import { Badge, Button, Modal } from "../ui";
import { cn } from "../../lib/cn";

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
    reload,
  } = usePlanSession(sessionId);

  const [subTab, setSubTab] = useState<PlanSubTab>("spec");

  // Action Modals State
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

  if (!plan) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
        <FileText className="size-10 text-subtle mb-3" />
        <h3 className="text-sm font-medium text-fg">未检测到计划规约</h3>
        <p className="mt-1 text-xs text-muted max-w-sm">
          可在对话中以「先规划」模式发送需求，或自动生成并审阅设计稿、契约与任务图。
        </p>
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
      setTimeout(() => {
        onApprove?.();
        onSwitchTab?.("chat");
      }, 600);
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
        <div className="flex items-center gap-2.5">
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
        {subTab === "spec" && <SpecEditor spec={plan.spec} onChange={updateSpec} />}
        {subTab === "contracts" && (
          <ContractsEditor
            contracts={plan.contracts}
            onUpdate={updateContract}
            onAdd={addContract}
            onRemove={removeContract}
          />
        )}
        {subTab === "tasks" && (
          <TasksTable
            tasks={plan.tasks}
            assignments={allocation?.assignments}
            validationIssues={validation?.issues}
            onUpdateTask={updateTask}
            onAddTask={addTask}
            onRemoveTask={removeTask}
            onReassign={(taskId, modelSpec) => {
              void runAllocation({ [taskId]: modelSpec });
            }}
          />
        )}
      </div>

      {/* Bottom Sticky Actions & Review Control Bar */}
      <div className="flex items-center justify-between border-t border-border bg-elevated px-5 py-3 shadow-lg">
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
            onClick={() => void handleApprove()}
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
            rows={4}
            value={feedbackText}
            onChange={(e) => setFeedbackText(e.target.value)}
            placeholder="例如：请把 T2 中的前端页面拆分为列表页和详情页两个任务，并补充相关的单元测试标准..."
            className="w-full rounded-md border border-border bg-elevated p-2.5 text-xs text-fg focus:border-accent focus:outline-hidden"
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
