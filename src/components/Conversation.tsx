import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bot,
  Brain,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  CircleX,
  Clock,
  ExternalLink,
  FileMinus2,
  FilePen,
  FilePlus2,
  Globe,
  Loader2,
  Pause,
  Play,
  Search,
  ShieldCheck,
  FileText,
  Terminal,
  Wrench,
} from "lucide-react";
import { cn } from "../lib/cn";
import { vendorLabel } from "../lib/models";
import type { TaskItem, ToolKind } from "../lib/ipc";
import { splitReply, useChat, type ApprovalPart, type ChatMessage, type FileChange, type Part, type ToolPart } from "../stores/chat";
import { useExecution } from "../stores/execution";
import { sessionCwd, useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { useUi } from "../stores/ui";
import { dispatchReadyTasks, dispatchSubAgentTask } from "../lib/taskRunner";
import { ApprovalCard } from "./ApprovalCard";
import { FullAutoBanner } from "./FullAutoBanner";
import { Markdown } from "./Markdown";
import { ProviderLogo } from "./ProviderLogo";

export function Conversation({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const messages = useChat((s) => s.conversations[sessionId]?.messages ?? []);
  const pipeline = useExecution((s) => s.pipelines[sessionId]);
  const hasPipeline = !!(pipeline?.tasks && pipeline.tasks.length > 0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const last = messages[messages.length - 1];
  const lastSize = (last?.text.length ?? 0) + (last?.parts?.length ?? 0) + (last?.files?.length ?? 0);

  // Follow the stream while a reply grows or pipeline starts, unless the user scrolled up to read.
  useEffect(() => {
    if (stick.current) endRef.current?.scrollIntoView({ block: "end" });
  }, [messages.length, lastSize, hasPipeline]);

  // m2c: Cold start / session switch detection: check if there is an active/persisted pipeline on disk
  const project = useProjects((s) => s.projects.find((p) => p.sessions.some((x) => x.id === sessionId)));
  const cwd = project && sessionId ? sessionCwd(project, sessionId) : null;
  const loadPipelineFromDisk = useExecution((s) => s.loadPipelineFromDisk);

  useEffect(() => {
    if (!pipeline && cwd && sessionId) {
      loadPipelineFromDisk(cwd, sessionId);
    }
  }, [sessionId, cwd, pipeline, loadPipelineFromDisk]);

  return (
    <div
      ref={scrollRef}
      role="log"
      aria-live="polite"
      aria-label="Conversation stream"
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
      }}
      className="min-h-0 flex-1 overflow-y-auto"
    >
      <FullAutoBanner sessionId={sessionId} />
      {messages.length === 0 && !hasPipeline ? (
        <div className="flex flex-1 items-center justify-center p-8 text-[13px] text-subtle">{t("workspace.chat.empty")}</div>
      ) : (
        <ol aria-label={t("workspace.tabs.chat")} className="mx-auto flex max-w-3xl flex-col gap-6 px-3 py-6 sm:px-6">
          {messages.map((m) => (
            <li key={m.id}>{m.role === "user" ? <UserBubble m={m} /> : <AssistantReply m={m} sessionId={sessionId} />}</li>
          ))}
          {hasPipeline && (
            <li key="plan-subagents-pipeline">
              <PlanSubagentsBanner sessionId={sessionId} />
            </li>
          )}
        </ol>
      )}
      <div ref={endRef} />
    </div>
  );
}

/** User messages longer than this (characters or lines) start folded. */
const FOLD_CHARS = 600;
const FOLD_LINES = 10;

function UserBubble({ m }: { m: ChatMessage }) {
  const { t } = useTranslation();
  const long = m.text.length > FOLD_CHARS || m.text.split("\n").length > FOLD_LINES;
  const [open, setOpen] = useState(false);
  const folded = long && !open;
  return (
    <div className="ml-auto flex w-fit max-w-[85%] flex-col items-end">
      <div
        data-selectable
        className={cn(
          "relative rounded-2xl bg-hover px-4 py-2.5 text-[14px] whitespace-pre-wrap text-fg",
          folded && "max-h-48 overflow-hidden",
        )}
      >
        {m.text}
        {folded && (
          <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-12 rounded-b-2xl bg-linear-to-t from-hover to-transparent" />
        )}
      </div>
      {long && (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          className="mt-1 inline-flex items-center gap-0.5 rounded px-1.5 text-xs text-muted hover:text-fg"
        >
          <ChevronRight aria-hidden className={cn("size-3 transition-transform", open ? "-rotate-90" : "rotate-90")} />
          {open ? t("workspace.chat.collapse") : t("workspace.chat.expand")}
        </button>
      )}
    </div>
  );
}

function useOpenFile(m: ChatMessage) {
  const openPreview = useUi((s) => s.openPreview);
  return (path: string, f?: FileChange) => {
    if (!m.cwd) return;
    const known = f ?? m.files?.find((x) => x.path === path || x.absPath === path);
    openPreview({ root: m.cwd, path: known?.previewPath ?? known?.absPath ?? path, diff: known?.diff ?? null, change: known?.change ?? null });
  };
}

function AssistantReply({ m, sessionId }: { m: ChatMessage; sessionId: string }) {
  const { t } = useTranslation();
  const streaming = m.status === "streaming";
  const provider = useProviders((s) => s.providers.find((p) => p.id === m.provider));
  const openFile = useOpenFile(m);
  const { process, answer } = splitReply(m.parts ?? []);
  const pendingApproval = m.parts?.find((p): p is ApprovalPart => p.type === "approval" && !p.resolved);
  // While the CLI is still working, the trailing text may be narration before the next step;
  // it's shown as the answer either way, and moves into the process when a step follows.
  const reported = m.reportedModel && m.reportedModel !== m.model ? m.reportedModel : null;

  return (
    <article aria-busy={streaming} className="text-[14px] text-fg">
      <header className="mb-2 flex flex-wrap items-center gap-x-1.5 gap-y-1 text-xs text-subtle">
        {m.provider && (
          <>
            <ProviderLogo id={m.provider} name={provider?.name ?? m.provider} className="size-5 rounded-md [&>*]:size-3.5" />
            <span className="font-medium text-muted">{provider?.name ?? vendorLabel(m.provider)}</span>
            <span aria-hidden>·</span>
            <span className="text-muted" title={m.model}>
              {m.modelName ?? m.model}
            </span>
            {reported && (
              <span title={t("workspace.chat.reportedModel")} className="font-mono text-[11px]">
                ({reported})
              </span>
            )}
          </>
        )}
        {m.mode && <span className="rounded bg-accent-soft px-1.5 text-accent">{t(`workspace.composer.modes.${m.mode}`)}</span>}
        {m.auto && <span className="rounded bg-hover px-1.5">{t("workspace.composer.modes.auto")}</span>}
      </header>

      {process.length > 0 && (
        <Process parts={process} m={m} live={streaming && !answer} onOpenFile={openFile} sessionId={sessionId} />
      )}

      {answer && <Markdown text={answer} onOpenFile={(p) => openFile(p)} className="text-[14px]" />}

      {pendingApproval && (
        <ApprovalCard p={pendingApproval} m={m} sessionId={sessionId} />
      )}

      {streaming && !answer && process.length === 0 && (
        <p role="status" className="inline-flex items-center gap-1.5 text-xs text-subtle">
          <Loader2 aria-hidden className="size-3.5 animate-spin" />
          {t("workspace.chat.thinking")}
        </p>
      )}

      {m.files && m.files.length > 0 && <FileList files={m.files} onOpen={(f) => openFile(f.path, f)} />}

      <Footer m={m} hasBodyStreaming={streaming && !answer && process.length === 0} />
    </article>
  );
}

function Footer({ m, hasBodyStreaming }: { m: ChatMessage; hasBodyStreaming?: boolean }) {
  const { t } = useTranslation();
  if (m.status === "waiting_approval") {
    return null;
  }
  if (m.status === "streaming" && !hasBodyStreaming) {
    return (
      <p role="status" className="mt-2 inline-flex items-center gap-1.5 text-xs text-subtle">
        <Loader2 aria-hidden className="size-3 animate-spin" />
        {t("workspace.chat.streaming")}
      </p>
    );
  }
  if (m.status === "error") {
    // If the quota step already displays this error, do not repeat it at the bottom (P3.m7c).
    const alreadyInQuota = m.parts?.some(
      (p) => p.type === "quota" && (m.error?.includes(p.raw) || p.raw.includes(m.error || "")),
    );
    if (alreadyInQuota) {
      return null;
    }
    return (
      <p role="alert" className="mt-2 flex items-start gap-1.5 text-xs text-danger">
        <CircleAlert aria-hidden className="mt-px size-3.5 shrink-0" />
        <span data-selectable className="whitespace-pre-wrap">
          {m.error}
        </span>
      </p>
    );
  }
  const u = m.usage;
  const bits = [
    m.status === "cancelled" ? t("workspace.chat.cancelled") : null,
    u && (u.input > 0 || u.output > 0)
      ? t("workspace.chat.usage", { input: u.input.toLocaleString(), output: u.output.toLocaleString() })
      : null,
    u?.costUsd ? `$${u.costUsd.toFixed(4)}` : null,
    u?.premiumRequests ? t("workspace.chat.premium", { count: u.premiumRequests }) : null,
    m.plan ? t("workspace.tokageDir.saved", { path: m.plan.path, version: m.plan.version }) : null,
    m.planError ? t("workspace.tokageDir.failed", { error: m.planError }) : null,
  ].filter(Boolean);
  if (bits.length === 0) return null;
  return <p className="mt-2 text-xs text-subtle tabular-nums">{bits.join(" · ")}</p>;
}

/** Reasoning, narration and tool steps leading up to the answer. Collapsed by default. */
function Process({
  parts,
  m,
  live,
  onOpenFile,
  sessionId,
}: {
  parts: Part[];
  m: ChatMessage;
  live: boolean;
  onOpenFile: (path: string) => void;
  sessionId: string;
}) {
  const { t } = useTranslation();
  const hasQuota = parts.some((p) => p.type === "quota");
  const quotaInterrupted = parts.some((p) => p.type === "quota" && !p.retrying);
  const [open, setOpen] = useState(() => quotaInterrupted);
  const tools = parts.filter((p): p is ToolPart => p.type === "tool");
  const failed = tools.filter((p) => p.ok === false).length;
  const thought = parts.some((p) => p.type === "reasoning");
  const current = [...parts].reverse().find((p) => p.type === "tool" && !p.done) as ToolPart | undefined;
  const seconds = m.startedAt && m.endedAt ? Math.max(1, Math.round((m.endedAt - m.startedAt) / 1000)) : null;
  const hasPendingApproval = parts.some((p) => p.type === "approval" && !p.resolved);

  const summary = hasPendingApproval
    ? t("workspace.chat.approval.badge")
    : quotaInterrupted
      ? t("workspace.chat.quota.badge")
      : live
      ? current
        ? t(`workspace.chat.running.${current.tool}`, { title: current.title })
        : thought
          ? t("workspace.chat.thinking")
          : t("workspace.chat.working")
      : [
          thought ? (seconds ? t("workspace.chat.thoughtFor", { seconds }) : t("workspace.chat.thought")) : null,
          tools.length ? t("workspace.chat.steps", { count: tools.length }) : null,
          failed ? t("workspace.chat.failedSteps", { count: failed }) : null,
          hasQuota ? t("workspace.chat.quota.stepCount") : null,
        ]
          .filter(Boolean)
          .join(" · ") || t("workspace.chat.process");

  return (
    <div className="mb-2">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="group inline-flex max-w-full items-center gap-1.5 rounded-md py-0.5 pr-1.5 text-[13px] text-muted hover:text-fg"
      >
        {live ? (
          <Loader2 aria-hidden className="size-3.5 shrink-0 animate-spin" />
        ) : (
          <Brain aria-hidden className="size-3.5 shrink-0" />
        )}
        <span className="truncate">{summary}</span>
        <ChevronRight aria-hidden className={cn("size-3.5 shrink-0 transition-transform", open && "rotate-90")} />
      </button>
      {open && (
        <div className="mt-1.5 ml-1.5 space-y-2 border-l border-border pl-3.5">
          {parts.map((p, i) =>
            p.type === "reasoning" ? (
              <div key={i} data-selectable className="text-[13px] leading-relaxed whitespace-pre-wrap text-muted italic">
                {p.text.trim()}
              </div>
            ) : p.type === "text" ? (
              <Markdown key={i} text={p.text} onOpenFile={onOpenFile} className="text-[13px] text-muted" />
            ) : p.type === "warning" ? (
              <div key={i} className="rounded-md border border-amber-500/40 bg-amber-500/10 p-2.5 text-[12px] text-amber-700 dark:text-amber-400 font-medium">
                ⚠️ {p.message}
              </div>
            ) : p.type === "quota" ? (
              <div
                key={i}
                className="rounded-md border border-red-500/40 bg-red-500/10 p-2.5 text-[12px] text-red-700 dark:text-red-400"
              >
                <div className="flex items-center justify-between font-medium">
                  <span className="flex items-center gap-1.5">
                    <CircleAlert className="size-3.5 text-danger" />
                    <span>{p.retrying ? t("workspace.chat.quota.retrying") : t("workspace.chat.quota.interrupted")}</span>
                  </span>
                  {p.resetAt && (
                    <span className="font-mono text-[11px] text-subtle">
                      {t("workspace.chat.quota.resetAt", { time: p.resetAt })}
                    </span>
                  )}
                </div>
                <div className="mt-1 font-mono text-[11.5px] whitespace-pre-wrap break-all opacity-90" data-selectable>
                  {p.raw}
                </div>
              </div>
            ) : p.type === "approval" ? (
              <ApprovalCard key={p.id || i} p={p} m={m} sessionId={sessionId} />
            ) : (
              <ToolRow key={p.id || i} p={p} m={m} />
            ),
          )}
        </div>
      )}
    </div>
  );
}

const TOOL_ICONS: Record<ToolKind, typeof Terminal> = {
  command: Terminal,
  read: FileText,
  search: Search,
  edit: FilePen,
  web: Globe,
  agent: Bot,
  other: Wrench,
};

function ToolRow({ p, m }: { p: ToolPart; m: ChatMessage }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const openFile = useOpenFile(m);
  const Icon = TOOL_ICONS[p.tool];
  const isAgent = p.tool === "agent";
  const expandable = isAgent || !!p.output || !!p.detail;

  return (
    <div className="text-xs">
      <div className="flex min-w-0 items-center gap-1.5 text-muted">
        <button
          type="button"
          disabled={!expandable}
          aria-expanded={expandable ? open : undefined}
          onClick={() => setOpen(!open)}
          className={cn(
            "flex min-w-0 flex-1 items-center gap-1.5 text-left transition-colors",
            expandable ? "cursor-pointer hover:text-fg" : "cursor-default"
          )}
        >
          <Icon aria-hidden className={cn("size-3.5 shrink-0", isAgent && "text-accent")} />
          <span className={cn("shrink-0", isAgent ? "font-semibold text-accent" : "text-subtle")}>
            {t(`workspace.chat.tools.${p.tool}`)}
          </span>
          <span className="truncate font-mono" title={p.title}>
            {p.title}
          </span>
          {isAgent && (
            <span className="shrink-0 text-[10px] text-muted rounded bg-base px-1 border border-border">
              {open ? "点击折叠" : "点击查看详情"}
            </span>
          )}
        </button>
        {p.files.map((f) => (
          <button
            key={f}
            type="button"
            onClick={() => openFile(f)}
            className="shrink-0 rounded px-1 font-mono text-accent hover:bg-hover"
          >
            {f.split("/").pop()}
          </button>
        ))}
        {!p.done ? (
          <Loader2 aria-label={t("workspace.chat.toolRunning")} className="size-3.5 shrink-0 animate-spin" />
        ) : p.ok === false ? (
          <CircleX aria-label={t("workspace.chat.toolFailed")} className="size-3.5 shrink-0 text-danger" />
        ) : p.ok === true ? (
          <CircleCheck aria-label={t("workspace.chat.toolOk")} className="size-3.5 shrink-0 text-success" />
        ) : null}
      </div>
      {open && (
        <div className="mt-1 ml-5 space-y-1">
          {p.detail && <p className="text-subtle">{p.detail}</p>}
          {p.output ? (
            <pre
              data-selectable
              className="max-h-72 overflow-auto rounded-md bg-sidebar px-2.5 py-2 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-muted border border-border/50"
            >
              {p.output}
            </pre>
          ) : isAgent ? (
            <p className="italic text-subtle text-[11px] py-1">
              {!p.done ? "子代理正在独立窗口中执行探查，实施步骤与产出将在完成后汇总呈现…" : "该子代理已完成，未产生额外标准输出。"}
            </p>
          ) : null}
        </div>
      )}
    </div>
  );
}

const CHANGE_ICONS = { added: FilePlus2, modified: FilePen, deleted: FileMinus2 };

/** "Created / edited / deleted" cards for the files a turn changed; each opens the preview. */
function FileList({ files, onOpen }: { files: FileChange[]; onOpen: (f: FileChange) => void }) {
  const { t } = useTranslation();
  const add = files.reduce((n, f) => n + f.additions, 0);
  const del = files.reduce((n, f) => n + f.deletions, 0);
  return (
    <section aria-label={t("workspace.chat.files.label")} className="mt-3 overflow-hidden rounded-xl border border-border">
      <header className="flex items-center justify-between bg-sidebar px-3 py-1.5 text-xs text-muted">
        <span>{t("workspace.chat.files.summary", { count: files.length })}</span>
        <Counts add={add} del={del} />
      </header>
      <ul className="divide-y divide-border">
        {files.map((f) => {
          const Icon = CHANGE_ICONS[f.change];
          const name = f.path.split(/[\\/]/).pop() ?? f.path;
          const dir = f.path.slice(0, f.path.length - name.length);
          return (
            <li key={f.absPath}>
              <button
                type="button"
                onClick={() => onOpen(f)}
                className="flex w-full items-center gap-2 px-3 py-2 text-left text-[13px] hover:bg-hover"
              >
                <Icon
                  aria-hidden
                  className={cn(
                    "size-4 shrink-0",
                    f.change === "added" && "text-success",
                    f.change === "deleted" && "text-danger",
                    f.change === "modified" && "text-warning",
                  )}
                />
                <span className="shrink-0 text-muted">{t(`workspace.chat.files.${f.change}`)}</span>
                <span className="min-w-0 truncate" title={f.path}>
                  <span className="font-medium">{name}</span>
                  {dir && <span className="ml-1.5 font-mono text-xs text-subtle">{dir}</span>}
                </span>
                <span className="ml-auto shrink-0">
                  <Counts add={f.additions} del={f.deletions} />
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function Counts({ add, del }: { add: number; del: number }) {
  const { t } = useTranslation();
  if (add === 0 && del === 0) return null;
  return (
    <span className="font-mono text-xs tabular-nums" aria-label={t("workspace.chat.files.lines", { add, del })}>
      <span className="text-success">+{add}</span> <span className="text-danger">−{del}</span>
    </span>
  );
}

/** Banner showing parallel subagents and queued tasks when plan execution is running. */
function PlanSubagentsBanner({ sessionId }: { sessionId: string }) {
  const pipeline = useExecution((s) => s.pipelines[sessionId]);
  const pauseExecution = useExecution((s) => s.pauseExecution);
  const resumeExecution = useExecution((s) => s.resumeExecution);
  const setWorkspaceTab = useUi((s) => s.setWorkspaceTab);
  const [collapsed, setCollapsed] = useState(false);

  if (!pipeline || !pipeline.tasks || pipeline.tasks.length === 0) return null;

  const updateTaskState = useExecution((s) => s.updateTaskState);
  const cwd = sessionCwd(useProjects.getState(), sessionId);
  const runningTasks = pipeline.tasks.filter((t) => pipeline.taskStates[t.id] === "RUNNING");
  const verifyingTasks = pipeline.tasks.filter((t) => pipeline.taskStates[t.id] === "VERIFYING");
  const interruptedTasks = pipeline.tasks.filter((t) => pipeline.taskStates[t.id] === "INTERRUPTED");
  const blockedTasks = pipeline.tasks.filter((t) => pipeline.taskStates[t.id] === "BLOCKED");
  const pendingTasks = pipeline.tasks.filter((t) => pipeline.taskStates[t.id] === "PENDING" || !pipeline.taskStates[t.id]);
  const doneTasks = pipeline.tasks.filter((t) => pipeline.taskStates[t.id] === "DONE");
  const isPaused = pipeline.status === "PAUSED";
  const isInterrupted = pipeline.status === "INTERRUPTED";
  const isAllDone = doneTasks.length === pipeline.tasks.length;

  const handleRetryTask = (t: TaskItem) => {
    updateTaskState(sessionId, t.id, "RUNNING", "[用户操作] 手动重试启动子代理任务。");
    const activePipeline = useExecution.getState().pipelines[sessionId];
    const projectCwd = cwd || activePipeline?.cwd || ".";
    void dispatchSubAgentTask({
      sessionId,
      task: t,
      contracts: activePipeline?.contracts ?? [],
      upstreamArtifacts: Object.values(activePipeline?.upstreamArtifacts ?? {}),
      baseCwd: projectCwd,
      useWorktree: true,
    });
  };

  return (
    <section
      aria-label="技术计划子代理执行看板"
      className="mx-auto my-3 max-w-3xl overflow-hidden rounded-xl border border-border bg-sidebar/50 shadow-xs"
    >
      <header className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-base px-3.5 py-2 text-xs">
        <div className="flex items-center gap-2">
          <Bot className="size-4 text-accent" />
          <span className="font-semibold text-fg">技术计划子代理执行看板</span>
          <span className="rounded bg-accent/15 px-1.5 py-0.5 text-[11px] font-medium text-accent">
            {isAllDone
              ? "全部完成"
              : isInterrupted
                ? "异常中断待恢复"
                : isPaused
                  ? "已暂停"
                  : blockedTasks.length > 0
                    ? `有阻塞任务待接管 (${blockedTasks.length})`
                    : `正在并行执行 (${runningTasks.length + verifyingTasks.length} 个子代理)`}
          </span>
          <span className="font-mono text-[11px] text-subtle">
            {doneTasks.length} / {pipeline.tasks.length} 完成
          </span>
        </div>

        <div className="flex items-center gap-1.5">
          {!isAllDone && (
            <button
              type="button"
              onClick={() => (isPaused ? resumeExecution(sessionId) : pauseExecution(sessionId))}
              className="inline-flex items-center gap-1 rounded border border-border bg-base px-2 py-0.5 text-[11px] font-medium text-muted hover:bg-hover hover:text-fg"
            >
              {isPaused ? <Play className="size-3 text-emerald-400" /> : <Pause className="size-3 text-amber-400" />}
              <span>{isPaused ? "继续执行" : "暂停调度"}</span>
            </button>
          )}

          <button
            type="button"
            onClick={() => setWorkspaceTab("pipeline")}
            className="inline-flex items-center gap-1 rounded border border-border bg-base px-2 py-0.5 text-[11px] font-medium text-muted hover:bg-hover hover:text-fg"
          >
            <ExternalLink className="size-3" />
            <span>查看流水线 DAG</span>
          </button>

          <button
            type="button"
            onClick={() => setCollapsed(!collapsed)}
            className="inline-flex items-center gap-0.5 rounded px-1.5 py-0.5 text-[11px] text-subtle hover:bg-hover hover:text-fg"
          >
            <ChevronRight className={cn("size-3.5 transition-transform", !collapsed && "rotate-90")} />
            <span>{collapsed ? "展开看板" : "收起"}</span>
          </button>
        </div>
      </header>

      {!collapsed && (
        <div className="divide-y divide-border/60 p-1">
          {/* Active Running Subagents */}
          {runningTasks.length > 0 && (
            <div className="space-y-1 p-2">
              <div className="flex items-center gap-1 px-1 text-[11px] font-semibold text-accent">
                <Loader2 className="size-3 animate-spin" />
                <span>并行作业中的子代理 ({runningTasks.length})</span>
              </div>
              <div className="space-y-1.5">
                {runningTasks.map((t) => (
                  <TaskAgentRow
                    key={t.id}
                    task={t}
                    state="RUNNING"
                    logs={pipeline.taskLogs[t.id] ?? []}
                    startedAt={pipeline.taskStartedAt?.[t.id]}
                    endedAt={pipeline.taskEndedAt?.[t.id]}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Verifying Subagents */}
          {verifyingTasks.length > 0 && (
            <div className="space-y-1 p-2">
              <div className="flex items-center gap-1 px-1 text-[11px] font-semibold text-indigo-400">
                <ShieldCheck className="size-3 animate-pulse" />
                <span>自动化验收关卡验证中 ({verifyingTasks.length})</span>
              </div>
              <div className="space-y-1.5">
                {verifyingTasks.map((t) => (
                  <TaskAgentRow
                    key={t.id}
                    task={t}
                    state="VERIFYING"
                    logs={pipeline.taskLogs[t.id] ?? []}
                    startedAt={pipeline.taskStartedAt?.[t.id]}
                    endedAt={pipeline.taskEndedAt?.[t.id]}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Blocked Subagents (Verification failed / Needs Takeover or Retry) */}
          {blockedTasks.length > 0 && (
            <div className="space-y-1 p-2">
              <div className="flex items-center gap-1 px-1 text-[11px] font-semibold text-rose-400">
                <CircleX className="size-3" />
                <span>已阻塞的任务，待人工处理或重新执行 ({blockedTasks.length})</span>
              </div>
              <div className="space-y-1.5">
                {blockedTasks.map((t) => (
                  <TaskAgentRow
                    key={t.id}
                    task={t}
                    state="BLOCKED"
                    logs={pipeline.taskLogs[t.id] ?? []}
                    startedAt={pipeline.taskStartedAt?.[t.id]}
                    endedAt={pipeline.taskEndedAt?.[t.id]}
                    onRetry={() => handleRetryTask(t)}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Interrupted Subagents (Cold start recovery / Crash) */}
          {interruptedTasks.length > 0 && (
            <div className="space-y-1 p-2">
              <div className="flex items-center gap-1 px-1 text-[11px] font-semibold text-amber-400">
                <CircleAlert className="size-3" />
                <span>异常中断待恢复的任务 ({interruptedTasks.length})</span>
              </div>
              <div className="space-y-1.5">
                {interruptedTasks.map((t) => (
                  <TaskAgentRow
                    key={t.id}
                    task={t}
                    state="INTERRUPTED"
                    logs={pipeline.taskLogs[t.id] ?? []}
                    startedAt={pipeline.taskStartedAt?.[t.id]}
                    endedAt={pipeline.taskEndedAt?.[t.id]}
                    onRetry={() => handleRetryTask(t)}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Pending Subagents in Queue */}
          {pendingTasks.length > 0 && (
            <div className="space-y-1 p-2">
              <div className="flex items-center gap-1 px-1 text-[11px] font-medium text-subtle">
                <Clock className="size-3" />
                <span>等待前置依赖的排队任务 ({pendingTasks.length})</span>
              </div>
              <div className="space-y-1.5">
                {pendingTasks.map((t) => (
                  <TaskAgentRow
                    key={t.id}
                    task={t}
                    state="PENDING"
                    logs={pipeline.taskLogs[t.id] ?? []}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Done Tasks */}
          {doneTasks.length > 0 && (
            <div className="space-y-1 p-2">
              <div className="flex items-center gap-1 px-1 text-[11px] font-medium text-emerald-400">
                <CircleCheck className="size-3" />
                <span>已完成并通过验收关卡 ({doneTasks.length})</span>
              </div>
              <div className="space-y-1.5">
                {doneTasks.map((t) => (
                  <TaskAgentRow
                    key={t.id}
                    task={t}
                    state="DONE"
                    logs={pipeline.taskLogs[t.id] ?? []}
                    startedAt={pipeline.taskStartedAt?.[t.id]}
                    endedAt={pipeline.taskEndedAt?.[t.id]}
                  />
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function formatTaskDuration(startedAt?: number, endedAt?: number): string | null {
  if (!startedAt) return null;
  const elapsed = (endedAt || Date.now()) - startedAt;
  if (elapsed < 0) return null;
  const totalSec = Math.floor(elapsed / 1000);
  const min = Math.floor(totalSec / 60);
  const sec = totalSec % 60;
  if (min === 0) return `${sec}s`;
  return `${min}m ${sec}s`;
}

function TaskAgentRow({
  task,
  state,
  logs,
  startedAt,
  endedAt,
  onRetry,
}: {
  task: TaskItem;
  state: "RUNNING" | "PENDING" | "DONE" | "INTERRUPTED" | "VERIFYING" | "BLOCKED" | "READY";
  logs: string[];
  startedAt?: number;
  endedAt?: number;
  onRetry?: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const mName = task.assigned_model ?? null;
  const durationText = formatTaskDuration(startedAt, endedAt);
  const latestLog = logs.length > 0 ? logs[logs.length - 1] : null;

  return (
    <div className="rounded-lg border border-border/80 bg-base p-2 text-xs transition-colors">
      <div className="flex items-center justify-between gap-2">
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          {state === "RUNNING" ? (
            <div className="flex size-5 shrink-0 items-center justify-center rounded-md bg-accent/15 text-accent">
              <Loader2 className="size-3.5 animate-spin" />
            </div>
          ) : state === "VERIFYING" ? (
            <div className="flex size-5 shrink-0 items-center justify-center rounded-md bg-indigo-500/15 text-indigo-400">
              <ShieldCheck className="size-3.5 animate-pulse" />
            </div>
          ) : state === "DONE" ? (
            <div className="flex size-5 shrink-0 items-center justify-center rounded-md bg-emerald-500/15 text-emerald-400">
              <CircleCheck className="size-3.5" />
            </div>
          ) : state === "BLOCKED" ? (
            <div className="flex size-5 shrink-0 items-center justify-center rounded-md bg-rose-500/15 text-rose-400">
              <CircleX className="size-3.5" />
            </div>
          ) : state === "INTERRUPTED" ? (
            <div className="flex size-5 shrink-0 items-center justify-center rounded-md bg-amber-500/15 text-amber-400">
              <CircleAlert className="size-3.5" />
            </div>
          ) : state === "READY" ? (
            <div className="flex size-5 shrink-0 items-center justify-center rounded-md bg-sky-500/15 text-sky-400">
              <Play className="size-3" />
            </div>
          ) : (
            <div className="flex size-5 shrink-0 items-center justify-center rounded-md bg-hover text-subtle">
              <Clock className="size-3.5" />
            </div>
          )}

          <div className="flex min-w-0 flex-1 items-center gap-1.5 truncate">
            <span className="font-mono font-bold text-accent shrink-0">{task.id}</span>
            <span className="truncate font-medium text-fg">{task.title}</span>
            <span className="shrink-0 rounded bg-hover px-1 py-0.5 text-[10px] text-subtle">
              {task.domain}
            </span>
            {state === "VERIFYING" && (
              <span className="shrink-0 rounded bg-indigo-500/20 px-1 py-0.5 text-[10px] font-medium text-indigo-400">
                验收关卡中
              </span>
            )}
            {state === "READY" && (
              <span className="shrink-0 rounded bg-sky-500/20 px-1 py-0.5 text-[10px] font-medium text-sky-400">
                就绪
              </span>
            )}
            {state === "BLOCKED" && (
              <span className="shrink-0 rounded bg-rose-500/20 px-1 py-0.5 text-[10px] font-medium text-rose-400">
                已阻塞
              </span>
            )}
            {state === "INTERRUPTED" && (
              <span className="shrink-0 rounded bg-amber-500/20 px-1 py-0.5 text-[10px] font-medium text-amber-400">
                异常中断
              </span>
            )}
          </div>

          {/* Duration timer */}
          {durationText && (
            <span className="shrink-0 font-mono text-[11px] tabular-nums text-subtle" title="任务执行耗时">
              {durationText}
            </span>
          )}

          <span className="shrink-0 rounded bg-elevated px-1.5 py-0.5 text-[10px] text-muted hover:text-fg">
            {expanded ? "收起日志" : "查看执行详情"}
          </span>
        </button>

        {/* Retry button if interrupted or blocked */}
        {(state === "INTERRUPTED" || state === "BLOCKED") && onRetry && (
          <button
            type="button"
            onClick={onRetry}
            className="inline-flex shrink-0 items-center gap-1 rounded bg-accent/15 px-2 py-0.5 text-[11px] font-medium text-accent hover:bg-accent/25"
          >
            <Play className="size-3" />
            <span>{state === "BLOCKED" ? "重新执行" : "继续执行"}</span>
          </button>
        )}

        {/* Assigned model badge */}
        {mName && (
          <div className="flex shrink-0 items-center gap-1 rounded border border-border/60 bg-elevated px-1.5 py-0.5 font-mono text-[11px] text-subtle">
            <Bot className="size-3 text-accent shrink-0" />
            <span className="truncate max-w-[130px]">{mName}</span>
          </div>
        )}
      </div>

      {/* Latest log/action preview when collapsed and running/verifying/blocked */}
      {!expanded && latestLog && (state === "RUNNING" || state === "VERIFYING" || state === "BLOCKED") && (
        <div className="mt-1.5 flex items-center gap-1.5 pl-7 text-[11px] text-muted truncate">
          <Terminal className="size-3 shrink-0 text-accent/80" />
          <span className="truncate font-mono">{latestLog}</span>
        </div>
      )}

      {/* Expanded Subagent details & logs */}
      {expanded && (
        <div className="mt-2 ml-7 space-y-1.5 pt-1 border-t border-border/40 text-[11px]">
          {task.goal && (
            <p className="text-muted leading-relaxed">
              <span className="font-semibold text-fg">任务目标: </span>
              {task.goal}
            </p>
          )}
          {task.depends_on && task.depends_on.length > 0 && (
            <p className="text-subtle font-mono">
              依赖任务: {task.depends_on.join(", ")}
            </p>
          )}
          {task.acceptance?.commands && task.acceptance.commands.length > 0 && (
            <div className="text-subtle font-mono">
              验收命令: {task.acceptance.commands.join(" · ")}
            </div>
          )}

          {logs.length > 0 && (
            <pre
              data-selectable
              className="max-h-56 overflow-auto rounded-md bg-sidebar px-2.5 py-2 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-muted border border-border/50"
            >
              {logs.join("\n")}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
