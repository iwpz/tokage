import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bot,
  Brain,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  CircleX,
  FileMinus2,
  FilePen,
  FilePlus2,
  Globe,
  Loader2,
  Search,
  FileText,
  Terminal,
  Wrench,
} from "lucide-react";
import { cn } from "../lib/cn";
import { vendorLabel } from "../lib/models";
import type { ToolKind } from "../lib/ipc";
import { splitReply, useChat, type ChatMessage, type FileChange, type Part, type ToolPart } from "../stores/chat";
import { useProviders } from "../stores/providers";
import { useUi } from "../stores/ui";
import { Markdown } from "./Markdown";
import { ProviderLogo } from "./ProviderLogo";

export function Conversation({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const messages = useChat((s) => s.conversations[sessionId]?.messages ?? []);
  const scrollRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const last = messages[messages.length - 1];
  const lastSize = (last?.text.length ?? 0) + (last?.parts?.length ?? 0) + (last?.files?.length ?? 0);

  // Follow the stream while a reply grows, unless the user scrolled up to read.
  useEffect(() => {
    if (stick.current) endRef.current?.scrollIntoView({ block: "end" });
  }, [messages.length, lastSize]);

  if (messages.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center p-8 text-[13px] text-subtle">{t("workspace.chat.empty")}</div>
    );
  }

  return (
    <div
      ref={scrollRef}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
      }}
      className="min-h-0 flex-1 overflow-y-auto"
    >
      <ol aria-label={t("workspace.tabs.chat")} className="mx-auto flex max-w-3xl flex-col gap-6 px-3 py-6 sm:px-6">
        {messages.map((m) => (
          <li key={m.id}>{m.role === "user" ? <UserBubble m={m} /> : <AssistantReply m={m} />}</li>
        ))}
      </ol>
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

function AssistantReply({ m }: { m: ChatMessage }) {
  const { t } = useTranslation();
  const streaming = m.status === "streaming";
  const provider = useProviders((s) => s.providers.find((p) => p.id === m.provider));
  const openFile = useOpenFile(m);
  const { process, answer } = splitReply(m.parts ?? []);
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

      {process.length > 0 && <Process parts={process} m={m} live={streaming && !answer} onOpenFile={openFile} />}

      {answer && <Markdown text={answer} onOpenFile={(p) => openFile(p)} className="text-[14px]" />}

      {streaming && !answer && process.length === 0 && (
        <p role="status" className="inline-flex items-center gap-1.5 text-xs text-subtle">
          <Loader2 aria-hidden className="size-3.5 animate-spin" />
          {t("workspace.chat.thinking")}
        </p>
      )}

      {m.files && m.files.length > 0 && <FileList files={m.files} onOpen={(f) => openFile(f.path, f)} />}

      <Footer m={m} />
    </article>
  );
}

function Footer({ m }: { m: ChatMessage }) {
  const { t } = useTranslation();
  if (m.status === "streaming") {
    return (
      <p role="status" className="mt-2 inline-flex items-center gap-1.5 text-xs text-subtle">
        <Loader2 aria-hidden className="size-3 animate-spin" />
        {t("workspace.chat.streaming")}
      </p>
    );
  }
  if (m.status === "error") {
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
}: {
  parts: Part[];
  m: ChatMessage;
  live: boolean;
  onOpenFile: (path: string) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const tools = parts.filter((p): p is ToolPart => p.type === "tool");
  const failed = tools.filter((p) => p.ok === false).length;
  const thought = parts.some((p) => p.type === "reasoning");
  const current = [...parts].reverse().find((p) => p.type === "tool" && !p.done) as ToolPart | undefined;
  const seconds = m.startedAt && m.endedAt ? Math.max(1, Math.round((m.endedAt - m.startedAt) / 1000)) : null;

  const summary = live
    ? current
      ? t(`workspace.chat.running.${current.tool}`, { title: current.title })
      : thought
        ? t("workspace.chat.thinking")
        : t("workspace.chat.working")
    : [
        thought ? (seconds ? t("workspace.chat.thoughtFor", { seconds }) : t("workspace.chat.thought")) : null,
        tools.length ? t("workspace.chat.steps", { count: tools.length }) : null,
        failed ? t("workspace.chat.failedSteps", { count: failed }) : null,
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
  const expandable = !!p.output || !!p.detail;

  return (
    <div className="text-xs">
      <div className="flex min-w-0 items-center gap-1.5 text-muted">
        <button
          type="button"
          disabled={!expandable}
          aria-expanded={expandable ? open : undefined}
          onClick={() => setOpen(!open)}
          className={cn("flex min-w-0 flex-1 items-center gap-1.5 text-left", expandable && "hover:text-fg")}
        >
          <Icon aria-hidden className="size-3.5 shrink-0" />
          <span className="shrink-0 text-subtle">{t(`workspace.chat.tools.${p.tool}`)}</span>
          <span className="truncate font-mono" title={p.title}>
            {p.title}
          </span>
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
          {p.output && (
            <pre
              data-selectable
              className="max-h-64 overflow-auto rounded-md bg-sidebar px-2.5 py-2 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-muted"
            >
              {p.output}
            </pre>
          )}
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
