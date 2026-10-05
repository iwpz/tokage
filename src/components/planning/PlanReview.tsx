import { useState } from "react";
import { CheckCircle2, AlertTriangle, Code2, Compass, FileText, ListTodo, RefreshCw, Save } from "lucide-react";
import { usePlanSession } from "./usePlanSession";
import { SpecEditor } from "./SpecEditor";
import { ContractsEditor } from "./ContractsEditor";
import { Badge, Button } from "../ui";
import { cn } from "../../lib/cn";

export type PlanSubTab = "spec" | "contracts" | "tasks";

export function PlanReview({ sessionId }: { sessionId: string }) {
  const {
    plan,
    loading,
    saving,
    dirty,
    error,
    validation,
    updateSpec,
    updateContract,
    addContract,
    removeContract,
    save,
    reload,
  } = usePlanSession(sessionId);

  const [subTab, setSubTab] = useState<PlanSubTab>("spec");

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

  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-base">
      {/* Top action & status bar */}
      <div className="flex items-center justify-between border-b border-border bg-elevated/60 px-5 py-2">
        <div className="flex items-center gap-1.5">
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
        {subTab === "spec" && (
          <SpecEditor spec={plan.spec} onChange={updateSpec} />
        )}
        {subTab === "contracts" && (
          <ContractsEditor
            contracts={plan.contracts}
            onUpdate={updateContract}
            onAdd={addContract}
            onRemove={removeContract}
          />
        )}
        {subTab === "tasks" && (
          <div className="flex h-full flex-col p-6 items-center justify-center text-center">
            <ListTodo className="size-10 text-subtle mb-3" />
            <h3 className="text-sm font-semibold text-fg">任务图表 (Task Graph)</h3>
            <p className="mt-1 text-xs text-muted max-w-md leading-relaxed">
              当前计划包含 {plan.tasks.length} 项任务。任务表、依赖关联与模型分配将在 P5.m5c 中全面审阅。
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
