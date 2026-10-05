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
  Table as TableIcon,
  Terminal,
  Workflow,
} from "lucide-react";
import {
  executeGateCommand,
  listAttempts,
  listGateRuns,
  type AttemptRow,
  type GateCommandResult,
  type GateRunRow,
  type MainSessionStatus,
  type TaskItem,
  type TaskState,
} from "../../lib/ipc";
import { usePlanSession } from "./usePlanSession";
import { Badge, Button, Modal } from "../ui";
import { cn } from "../../lib/cn";

export function PipelineView({
  sessionId,
  onSwitchTab,
}: {
  sessionId: string;
  onSwitchTab?: (tab: any) => void;
}) {
  const { plan, loading } = usePlanSession(sessionId);
  const [sessionStatus, setSessionStatus] = useState<MainSessionStatus>("RUNNING");
  const [taskStates, setTaskStates] = useState<Record<string, TaskState>>({});
  const [, setAttempts] = useState<AttemptRow[]>([]);
  const [, setGateRuns] = useState<GateRunRow[]>([]);
  const [selectedTask, setSelectedTask] = useState<TaskItem | null>(null);
  const [displayMode, setDisplayMode] = useState<"dag" | "table">("dag");

  // Takeover Modal
  const [takeoverModalOpen, setTakeoverModalOpen] = useState(false);
  const [takeoverTask, setTakeoverTask] = useState<TaskItem | null>(null);
  const [resumeCmd, setResumeCmd] = useState("");

  // Gate execution state for manual / verified testing
  const [runningGate, setRunningGate] = useState(false);
  const [gateOutput, setGateOutput] = useState<string | null>(null);

  useEffect(() => {
    if (plan?.tasks) {
      const initial: Record<string, TaskState> = {};
      plan.tasks.forEach((t) => {
        initial[t.id] = t.depends_on.length === 0 ? "READY" : "PENDING";
      });
      setTaskStates(initial);
      if (plan.tasks.length > 0 && !selectedTask) {
        setSelectedTask(plan.tasks[0]);
      }
    }
  }, [plan]);

  useEffect(() => {
    if (sessionId) {
      listAttempts(sessionId).then(setAttempts).catch(() => {});
      listGateRuns(undefined, "plan-1").then(setGateRuns).catch(() => {});
    }
  }, [sessionId]);

  if (loading) {
    return (
      <div className="flex flex-1 items-center justify-center p-8 text-xs text-muted">
        正在加载流水线状态...
      </div>
    );
  }

  if (!plan || plan.tasks.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
        <Workflow className="size-10 text-subtle mb-3" />
        <h3 className="text-sm font-medium text-fg">无可用流水线</h3>
        <p className="mt-1 text-xs text-muted">请先在「规划」标签页审阅并确认计划。</p>
      </div>
    );
  }

  const tasks = plan.tasks;
  const runningCount = Object.values(taskStates).filter((s) => s === "RUNNING").length;
  const doneCount = Object.values(taskStates).filter((s) => s === "DONE").length;
  const blockedCount = Object.values(taskStates).filter((s) => s === "BLOCKED").length;

  const handleSimulateStatus = (taskId: string, nextState: TaskState) => {
    setTaskStates((prev) => {
      const updated = { ...prev, [taskId]: nextState };
      // If done, unlock downstream
      if (nextState === "DONE") {
        tasks.forEach((t) => {
          if (updated[t.id] === "PENDING") {
            const allDepsDone = t.depends_on.every((d) => updated[d] === "DONE");
            if (allDepsDone) {
              updated[t.id] = "READY";
            }
          }
        });
      }
      return updated;
    });
  };

  const handleRunGate = async (task: TaskItem) => {
    if (task.acceptance.commands.length === 0) return;
    setRunningGate(true);
    setGateOutput("正在执行验收命令...");
    try {
      const cmd = task.acceptance.commands[0];
      const res: GateCommandResult = await executeGateCommand(cmd, ".", 30, true);
      setGateOutput(res.outputTail || (res.passed ? "关卡命令执行成功 (0)" : "关卡命令失败"));
      if (res.passed) {
        handleSimulateStatus(task.id, "DONE");
      } else {
        handleSimulateStatus(task.id, "REPAIRING");
      }
    } catch (e: any) {
      setGateOutput(`执行异常: ${e}`);
    } finally {
      setRunningGate(false);
    }
  };

  const openTakeover = (task: TaskItem) => {
    setTakeoverTask(task);
    setResumeCmd(`claude --resume ${sessionId}-${task.id}`);
    setTakeoverModalOpen(true);
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
              onClick={() => setSessionStatus("PAUSED")}
              className="gap-1.5 text-xs"
            >
              <Pause className="size-3.5" />
              暂停调度
            </Button>
          ) : (
            <Button
              variant="primary"
              size="sm"
              onClick={() => setSessionStatus("RUNNING")}
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
                    <th scope="col" className="p-2.5">写入范围</th>
                    <th scope="col" className="p-2.5 text-right">操作</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/60">
                  {tasks.map((task) => {
                    const state = taskStates[task.id] || "PENDING";
                    const isSelected = selectedTask?.id === task.id;
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
                          <TaskStateBadge state={state} />
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
                          {task.write_scope.length > 0 ? task.write_scope[0] : "—"}
                        </td>
                        <td className="p-2.5 text-right">
                          {state === "READY" && (
                            <Button
                              variant="secondary"
                              size="sm"
                              className="h-6 text-[11px] px-2"
                              onClick={(e) => {
                                e.stopPropagation();
                                handleSimulateStatus(task.id, "RUNNING");
                              }}
                            >
                              执行
                            </Button>
                          )}
                          {state === "BLOCKED" && (
                            <Button
                              variant="secondary"
                              size="sm"
                              className="h-6 text-[11px] px-2 text-danger hover:text-danger"
                              onClick={(e) => {
                                e.stopPropagation();
                                openTakeover(task);
                              }}
                            >
                              接管
                            </Button>
                          )}
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
                    </div>

                    <div className="flex items-center gap-2">
                      <TaskStateBadge state={state} />
                    </div>
                  </div>

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
                          onClick={() => handleSimulateStatus(task.id, "RUNNING")}
                        >
                          开始执行
                        </Button>
                      )}
                      {state === "RUNNING" && (
                        <Button
                          variant="secondary"
                          size="sm"
                          className="h-6 text-[11px] px-2 text-emerald-500 hover:text-emerald-600"
                          onClick={() => handleSimulateStatus(task.id, "DONE")}
                        >
                          标记完成
                        </Button>
                      )}
                      {state === "BLOCKED" && (
                        <Button
                          variant="secondary"
                          size="sm"
                          className="h-6 text-[11px] px-2 gap-1 border-danger text-danger hover:bg-danger/10"
                          onClick={() => openTakeover(task)}
                        >
                          <Terminal className="size-3" />
                          终端接管
                        </Button>
                      )}
                    </div>
                  </div>
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
                  {selectedTask.acceptance.commands.length > 0 && (
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={runningGate}
                      onClick={() => handleRunGate(selectedTask)}
                      className="h-6 text-[11px] px-2"
                    >
                      {runningGate ? "运行中..." : "执行关卡"}
                    </Button>
                  )}
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

                {gateOutput && (
                  <div className="mt-2 text-[11px] font-mono bg-bg/80 p-2 rounded border border-border whitespace-pre-wrap max-h-36 overflow-y-auto text-muted">
                    {gateOutput}
                  </div>
                )}
              </div>

              {/* State Transition Testing Controls */}
              <div className="rounded-md border border-border p-3 bg-subtle/10 space-y-2">
                <span className="text-xs font-semibold text-fg flex items-center gap-1.5">
                  <Activity className="size-3.5 text-accent" />
                  状态推进控制
                </span>
                <div className="grid grid-cols-2 gap-1.5">
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-[11px] border border-border justify-start"
                    onClick={() => handleSimulateStatus(selectedTask.id, "READY")}
                  >
                    置为 READY
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-[11px] border border-border justify-start text-accent"
                    onClick={() => handleSimulateStatus(selectedTask.id, "RUNNING")}
                  >
                    置为 RUNNING
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-[11px] border border-border justify-start text-amber-500"
                    onClick={() => handleSimulateStatus(selectedTask.id, "REPAIRING")}
                  >
                    触发 REPAIR
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-[11px] border border-border justify-start text-danger"
                    onClick={() => handleSimulateStatus(selectedTask.id, "BLOCKED")}
                  >
                    标记 BLOCKED
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-[11px] border border-border justify-start col-span-2 text-emerald-500"
                    onClick={() => handleSimulateStatus(selectedTask.id, "DONE")}
                  >
                    通过验收 (DONE)
                  </Button>
                </div>
              </div>
            </>
          ) : (
            <div className="flex flex-1 items-center justify-center text-xs text-subtle">
              请选择一个任务查看详情
            </div>
          )}
        </div>
      </div>

      {/* Terminal Takeover Modal */}
      <Modal
        open={takeoverModalOpen}
        onClose={() => setTakeoverModalOpen(false)}
        title={`终端接管任务: ${takeoverTask?.id}`}
      >
        <div className="space-y-4 text-xs">
          <p className="text-muted leading-relaxed">
            该任务多次自动修复均未通过，当前已置为 <strong>BLOCKED</strong>。您可以在终端中直接接管此 CLI 会话，人工调试并修改代码：
          </p>
          <div className="bg-bg p-3 rounded-md font-mono text-[12px] border border-border text-fg select-all">
            {resumeCmd}
          </div>
          <p className="text-[11px] text-muted">
            在外部终端完成修改后，点击下方「已完成接管」，Tokage 将自动重跑该任务的验收关卡。
          </p>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={() => setTakeoverModalOpen(false)}>
              稍后处理
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                if (takeoverTask) {
                  handleSimulateStatus(takeoverTask.id, "DONE");
                }
                setTakeoverModalOpen(false);
              }}
            >
              已完成接管 (重跑关卡)
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
    default:
      return <Badge variant="muted">{state}</Badge>;
  }
}
