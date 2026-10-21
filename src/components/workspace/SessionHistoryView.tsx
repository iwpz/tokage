import { useEffect, useMemo, useState, useRef, useId } from "react";
import { useTranslation } from "react-i18next";
import {
  Bot,
  Brain,
  ChevronDown,
  ChevronRight,
  Clock,
  Cpu,
  FolderOpen,
  Hash,
  Layers,
  MessageSquare,
  RefreshCw,
  Search,
  Sparkles,
  Terminal,
  User,
  X,
} from "lucide-react";
import { pollHistory } from "../../lib/historyPoller";
import { equalOrderedSnapshots } from "../../lib/historySnapshot";
import { useTaskTranscripts, transcriptRows, flushTaskTranscript } from "../../stores/taskTranscripts";
import { cn } from "../../lib/cn";
import i18n from "../../i18n";
import {
  getChatLogs,
  listAttempts,
  type AttemptRow,
  type ChatLogRow,
  type ProviderId,
} from "../../lib/ipc";
import { ProviderLogo } from "../ProviderLogo";
import { Markdown } from "../Markdown";
import { Badge, Button } from "../ui";

export function SessionHistoryView({
  sessionId,
  projectId,
}: {
  sessionId?: string | null;
  projectId?: string | null;
}) {
  const { t } = useTranslation();
  const viewId = useId();
  const [scope, setScope] = useState<"session" | "project">("session");
  const [attempts, setAttempts] = useState<AttemptRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedAttemptId, setSelectedAttemptId] = useState<string | null>(null);
  const [chatLogs, setChatLogs] = useState<ChatLogRow[]>([]);
  const [loadingLogs, setLoadingLogs] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [expandedClis, setExpandedClis] = useState<Record<string, boolean>>({});
  const [expandedModels, setExpandedModels] = useState<Record<string, boolean>>({});
  const transcriptRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const followLatestRef = useRef(true);
  const [showLatest, setShowLatest] = useState(false);

  const [readError, setReadError] = useState(false);
  const [attemptError, setAttemptError] = useState(false);
  const attemptReader = useRef<ReturnType<typeof pollHistory<AttemptRow[]>> | null>(null);
  const logReader = useRef<ReturnType<typeof pollHistory<ChatLogRow[]>> | null>(null);
  const live = useTaskTranscripts((state) => selectedAttemptId ? state.entries[selectedAttemptId] : undefined);
  // In-memory event updates win over an older SQLite snapshot of the same message.
  const visibleLogs = live ? transcriptRows(live) : chatLogs;

  useEffect(() => {
    setAttempts([]); setSelectedAttemptId(null); setChatLogs([]); setLoading(true); setAttemptError(false);
    const reader = pollHistory(
      () => scope === "session" ? (sessionId ? listAttempts(sessionId) : Promise.resolve([])) : (projectId ? listAttempts(undefined, projectId) : Promise.resolve([])),
      (data) => {
        setAttempts((current) => equalOrderedSnapshots(current, data) ? current : data); setLoading(false); setAttemptError(false);
        setSelectedAttemptId((selected) => data.some((a) => a.id === selected) ? selected : data[0]?.id ?? null);
      },
      () => { setLoading(false); setAttemptError(true); },
    );
    attemptReader.current = reader;
    return () => { reader.stop(); attemptReader.current = null; };
  }, [sessionId, projectId, scope]);

  useEffect(() => {
    setChatLogs([]); setReadError(false);
    followLatestRef.current = true;
    setShowLatest(false);
    if (!selectedAttemptId) { setLoadingLogs(false); return; }
    setLoadingLogs(true);
    const reader = pollHistory(() => getChatLogs(selectedAttemptId),
      (logs) => { setChatLogs((current) => equalOrderedSnapshots(current, logs) ? current : logs); setLoadingLogs(false); setReadError(false); },
      () => { setLoadingLogs(false); setReadError(true); });
    logReader.current = reader;
    return () => { reader.stop(); logReader.current = null; };
  }, [selectedAttemptId]);

  useEffect(() => {
    const container = transcriptRef.current;
    if (!container || !selectedAttemptId) return;
    let active = true;
    const update = () => {
      if (!active || transcriptRef.current !== container) return;
      const atBottom = container.scrollHeight - container.scrollTop - container.clientHeight <= 32;
      if (followLatestRef.current || atBottom) {
        followLatestRef.current = true;
        container.scrollTop = container.scrollHeight;
        setShowLatest(false);
      } else setShowLatest(true);
    };
    const observer = new ResizeObserver(update);
    observer.observe(container);
    Array.from(container.children).forEach((child) => observer.observe(child));
    const mutations = new MutationObserver((records) => {
      if (!active) return;
      for (const record of records) {
        record.removedNodes.forEach((node) => {
          if (node instanceof Element) {
            observer.unobserve(node);
            node.querySelectorAll("*").forEach((child) => observer.unobserve(child));
          }
        });
        record.addedNodes.forEach((node) => {
          if (node instanceof Element) {
            observer.observe(node);
            node.querySelectorAll("*").forEach((child) => observer.observe(child));
          }
        });
      }
    });
    mutations.observe(container, { childList: true, subtree: true });
    update();
    return () => {
      active = false;
      mutations.disconnect();
      observer.disconnect();
    };
  }, [selectedAttemptId]);

  const returnToLatest = () => {
    const container = transcriptRef.current;
    if (!container) return;
    followLatestRef.current = true;
    container.scrollTop = container.scrollHeight;
    setShowLatest(false);
  };

  const refresh = () => {
    setLoading(true);
    setAttemptError(false);
    void attemptReader.current?.refresh(); void logReader.current?.refresh();
    if (selectedAttemptId) void flushTaskTranscript(selectedAttemptId);
  };

  const searchActive = searchQuery.trim() !== "";

  // Group attempts into CLI -> Model -> Attempts two-level tree
  const tree = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    const filtered = attempts.filter((a) => {
      if (!q) return true;
      const displayedProvider = formatCliName(historyProvider(a));
      const displayedRole = t(`sessionHistory.roles.${a.role}`, { defaultValue: a.role });
      return (
        String(a.taskId ?? "").toLowerCase().includes(q) ||
        a.status.toLowerCase().includes(q) ||
        displayedProvider.toLowerCase().includes(q) ||
        displayedRole.toLowerCase().includes(q) ||
        a.providerName.toLowerCase().includes(q) ||
        a.modelId.toLowerCase().includes(q) ||
        a.role.toLowerCase().includes(q) ||
        a.id.toLowerCase().includes(q)
      );
    });

    const cliMap: Record<
      string,
      Record<string, AttemptRow[]>
    > = {};

    for (const a of filtered) {
      const cli = historyProvider(a);
      const model = a.modelId || (isNonModelHistory(a) ? t("sessionHistory.noModel") : "default");
      if (!cliMap[cli]) cliMap[cli] = {};
      if (!cliMap[cli][model]) cliMap[cli][model] = [];
      cliMap[cli][model].push(a);
    }

    return cliMap;
  }, [attempts, searchQuery, t]);

  // Automatically expand all nodes on initial load
  useEffect(() => {
    const clis: Record<string, boolean> = {};
    const models: Record<string, boolean> = {};
    for (const cli of Object.keys(tree)) {
      clis[cli] = true;
      for (const m of Object.keys(tree[cli])) {
        models[`${cli}::${m}`] = true;
      }
    }
    setExpandedClis((prev) => ({ ...clis, ...prev }));
    setExpandedModels((prev) => ({ ...models, ...prev }));
  }, [tree]);

  const toggleCli = (cli: string) => {
    setExpandedClis((prev) => ({ ...prev, [cli]: !prev[cli] }));
  };

  const toggleModel = (key: string) => {
    setExpandedModels((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  const selectedAttempt = useMemo(() => {
    return attempts.find((a) => a.id === selectedAttemptId) ?? null;
  }, [attempts, selectedAttemptId]);

  const isDebugAttempt = (attempt: { debugOutput?: string | null }) => !!attempt.debugOutput;

  return (
    <div className="flex flex-1 min-h-0 bg-base overflow-hidden">
      {/* Left Tree & Session Navigation */}
      <div className="flex w-80 shrink-0 flex-col border-r border-border bg-sidebar select-none">
        {/* Top filter and scope switch */}
        <div className="flex flex-col gap-2 border-b border-border p-3 bg-elevated/40">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold text-fg flex items-center gap-1.5">
              <Layers className="size-3.5 text-accent" />
              <span>{t("sessionHistory.title")}</span>
            </span>
            <div className="flex items-center gap-1">
              <Button
                size="sm"
                variant="ghost"
                onClick={refresh}
                title={t("sessionHistory.refreshTitle")}
                className="size-6 p-0 text-muted hover:text-fg"
              >
                <RefreshCw className={cn("size-3", loading && "animate-spin text-accent")} />
              </Button>
            </div>
          </div>

          {/* Scope Segmented Control */}
          <div className="flex rounded-md bg-active/40 p-0.5 border border-border/60">
            <button
              type="button"
              onClick={() => setScope("session")}
              aria-pressed={scope === "session"}
              className={cn(
                "flex-1 rounded py-1 text-[11px] font-medium transition-colors text-center",
                scope === "session"
                  ? "bg-elevated text-fg shadow-xs font-semibold"
                  : "text-muted hover:text-fg",
              )}
            >
              {t("sessionHistory.scopeCurrent")}
            </button>
            <button
              type="button"
              onClick={() => setScope("project")}
              aria-pressed={scope === "project"}
              className={cn(
                "flex-1 rounded py-1 text-[11px] font-medium transition-colors text-center",
                scope === "project"
                  ? "bg-elevated text-fg shadow-xs font-semibold"
                  : "text-muted hover:text-fg",
              )}
            >
              {t("sessionHistory.scopeProject")}
            </button>
          </div>

          {/* Search box */}
          <div className="relative">
            <Search className="absolute left-2.5 top-2 size-3 text-subtle" />
            <input
              ref={searchInputRef}
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  setSearchQuery("");
                  searchInputRef.current?.focus();
                }
              }}
              aria-label={t("sessionHistory.searchLabel")}
              placeholder={t("sessionHistory.searchPlaceholder")}
              className="w-full rounded-md border border-border bg-elevated pl-7 pr-7 py-1 text-xs text-fg placeholder:text-subtle focus:border-accent focus:outline-hidden"
            />
            {searchQuery.length > 0 && (
              <button
                type="button"
                aria-label={t("sessionHistory.clearSearch")}
                title={t("sessionHistory.clearSearch")}
                onClick={() => {
                  setSearchQuery("");
                  searchInputRef.current?.focus();
                }}
                className="absolute right-1 top-0.5 flex size-5 items-center justify-center rounded text-subtle hover:text-fg focus:outline-hidden focus:ring-1 focus:ring-accent"
              >
                <X className="size-3" />
              </button>
            )}
          </div>
        </div>

        {/* Tree List */}
        <div className="flex-1 overflow-y-auto p-2 space-y-1">
          {loading && attempts.length > 0 && <div role="status" aria-live="polite" className="px-2 py-1 text-xs text-muted">{t("sessionHistory.loading")}</div>}
          {attemptError && attempts.length > 0 && (
            <div className="flex items-center justify-between gap-2 px-2 py-1 text-xs text-amber-400">
              <p role="alert">{t("sessionHistory.attemptReadError")}</p>
              <Button size="sm" variant="ghost" onClick={refresh}>{t("sessionHistory.retry")}</Button>
            </div>
          )}
          {loading && attempts.length === 0 ? (
            <div role="status" aria-live="polite" className="p-4 text-center text-xs text-muted">{t("sessionHistory.loading")}</div>
          ) : attempts.length === 0 && attemptError ? (
            <div className="flex flex-col items-center gap-2 p-4 text-center text-xs text-muted">
              <p role="alert">{t("sessionHistory.attemptReadError")}</p>
              <Button size="sm" variant="ghost" onClick={refresh}>{t("sessionHistory.retry")}</Button>
            </div>
          ) : attempts.length === 0 ? (
            <div className="flex flex-col items-center justify-center p-6 text-center text-xs text-muted">
              <Bot className="size-8 text-subtle mb-2" />
              <span>{t("sessionHistory.emptyTitle")}</span>
              <p className="mt-1 text-[11px] text-subtle max-w-[200px]">
                {t("sessionHistory.emptyBody")}
              </p>
            </div>
          ) : Object.keys(tree).length === 0 ? (
            <div className="p-4 text-center text-xs text-subtle">{t("sessionHistory.noMatch")}</div>
          ) : (
            Object.entries(tree).map(([cli, models]) => {
              const cliExpanded = searchActive || (expandedClis[cli] ?? true);
              const cliPanelId = `${viewId}-provider-${encodeURIComponent(cli)}`;
              const cliTotalAttempts = Object.values(models).reduce(
                (sum, arr) => sum + arr.length,
                0,
              );

              return (
                <div key={cli} className="rounded-md border border-border/40 bg-elevated/20 overflow-hidden">
                  {/* Level 1: CLI Row */}
                  <button
                    type="button"
                    onClick={() => toggleCli(cli)}
                    aria-expanded={cliExpanded}
                    aria-controls={cliPanelId}
                    disabled={searchActive}
                    className="flex w-full items-center justify-between px-2.5 py-1.5 hover:bg-hover/80 text-left transition-colors"
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      {cliExpanded ? (
                        <ChevronDown className="size-3 text-subtle shrink-0" />
                      ) : (
                        <ChevronRight className="size-3 text-subtle shrink-0" />
                      )}
                      <ProviderLogo
                        id={cli as ProviderId}
                        name={cli}
                        className="size-4.5 rounded"
                      />
                      <span className="truncate text-xs font-semibold text-fg">
                        {formatCliName(cli)}
                      </span>
                    </div>
                    <Badge variant="muted" size="xs" className="shrink-0 text-[10px]">
                      {cliTotalAttempts}
                    </Badge>
                  </button>

                  {/* Level 2: Models under this CLI */}
                  <div id={cliPanelId} hidden={!cliExpanded} className="pl-3 pr-1 pb-1 space-y-1 border-t border-border/30 bg-base/40">
                      {Object.entries(models).map(([model, modelAttempts]) => {
                        const modelKey = `${cli}::${model}`;
                        const modelExpanded = searchActive || (expandedModels[modelKey] ?? true);
                        const modelPanelId = `${viewId}-model-${encodeURIComponent(modelKey)}`;
                        const isDebug = modelAttempts.some(isDebugAttempt);

                        return (
                          <div key={modelKey} className="rounded border border-border/30 bg-elevated/40 mt-1">
                            {/* Model Row */}
                            <button
                              type="button"
                              onClick={() => toggleModel(modelKey)}
                              aria-expanded={modelExpanded}
                              aria-controls={modelPanelId}
                              disabled={searchActive}
                              className="flex w-full items-center justify-between px-2 py-1 hover:bg-hover text-left transition-colors"
                            >
                              <div className="flex items-center gap-1.5 min-w-0">
                                {modelExpanded ? (
                                  <ChevronDown className="size-2.5 text-subtle shrink-0" />
                                ) : (
                                  <ChevronRight className="size-2.5 text-subtle shrink-0" />
                                )}
                                <Cpu className="size-3 text-muted shrink-0" />
                                <span className="truncate text-[11px] font-medium text-fg">
                                  {model}
                                </span>
                                {isDebug && (
                                  <span className="inline-flex items-center rounded bg-amber-500/15 border border-amber-500/30 px-1 py-0.2 text-[9px] font-medium text-amber-400 shrink-0">
                                    {t("debugProvider.title")}
                                  </span>
                                )}
                              </div>
                              <span className="text-[10px] text-muted font-mono shrink-0">
                                {modelAttempts.length}
                              </span>
                            </button>

                            {/* Level 3: Attempts list */}
                            <div id={modelPanelId} hidden={!modelExpanded} className="pl-2 pr-1 pb-1 space-y-0.5">
                                {modelAttempts.map((att) => {
                                  const isSelected = att.id === selectedAttemptId;
                                  const timeStr = att.createdAt
                                    ? formatTime(att.createdAt)
                                    : t("sessionHistory.unknownTime");

                                  return (
                                    <button
                                      key={att.id}
                                      data-attempt-id={att.id}
                                      aria-current={isSelected ? "true" : undefined}
                                      type="button"
                                      onClick={() => setSelectedAttemptId(att.id)}
                                      className={cn(
                                        "flex w-full items-center justify-between rounded px-2 py-1 text-left text-[11px] transition-colors",
                                        isSelected
                                          ? "bg-active text-fg font-medium shadow-xs"
                                          : "text-muted hover:bg-hover hover:text-fg",
                                      )}
                                    >
                                      <div className="flex items-center gap-1.5 min-w-0">
                                        {!isNonModelHistory(att) && <RoleBadge role={att.role} />}
                                        <span className="truncate text-[10px] font-mono">
                                          {timeStr}
                                        </span>
                                      </div>
                                      <span className="text-[9px] text-subtle font-mono">
                                        {t("sessionHistory.turns", { count: att.turnCount })}
                                      </span>
                                    </button>
                                  );
                                })}
                            </div>
                          </div>
                        );
                      })}
                  </div>
                </div>
              );
            })
          )}
        </div>
      </div>

      {/* Right Detail Pane */}
      <div className="flex flex-1 flex-col overflow-hidden bg-base">
        {selectedAttempt ? (
          <>
            {/* Attempt Details Header Bar */}
            <div className="flex flex-col border-b border-border bg-elevated/60 px-5 py-3 gap-2 shrink-0">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2.5 flex-wrap">
                  <span className="text-sm font-semibold text-fg flex items-center gap-1.5">
                    <ProviderLogo
                      id={selectedAttempt.providerName as ProviderId}
                      name={selectedAttempt.providerName}
                      className="size-5 rounded"
                    />
                    <span>{formatCliName(historyProvider(selectedAttempt))}</span>
                  </span>
                  <span className="text-muted">/</span>
                  <span className="font-mono text-xs font-semibold text-accent">
                    {selectedAttempt.modelId || (isNonModelHistory(selectedAttempt) ? t("sessionHistory.noModel") : "default")}
                  </span>
                  {isDebugAttempt(selectedAttempt) && (
                    <Badge variant="warning" size="xs" className="gap-1 font-semibold">
                      <Sparkles className="size-3" />
                      <span>{t("debugProvider.title")}</span>
                    </Badge>
                  )}
                  {!isNonModelHistory(selectedAttempt) && <RoleBadge role={selectedAttempt.role} />}
                  <Badge variant="muted" size="xs" className="font-mono text-[10px]">
                    {selectedAttempt.status}
                  </Badge>
                </div>

                <div className="flex items-center gap-2">
                  <span className="text-xs text-subtle font-mono">
                    ID: {selectedAttempt.id.slice(0, 18)}...
                  </span>
                </div>
              </div>

              {/* Debug execution provenance */}
              {isDebugAttempt(selectedAttempt) && (
                <div className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-1.5 text-xs text-amber-300 flex items-center gap-2">
                  <Sparkles className="size-3.5 shrink-0 text-amber-400" />
                  <span>
                    <strong>{t("sessionHistory.debugBanner")}</strong>
                    {t("debugProvider.history", { source: selectedAttempt.debugOutput?.toUpperCase() })}
                  </span>
                </div>
              )}

              {/* Meta metrics grid */}
              <div className="flex items-center gap-4 text-xs text-muted flex-wrap">
                <span className="flex items-center gap-1">
                  <Clock className="size-3 text-subtle" />
                  <span>
                    {t("sessionHistory.createdAt", {
                      time: selectedAttempt.createdAt
                        ? new Date(selectedAttempt.createdAt).toLocaleString()
                        : t("sessionHistory.unknown"),
                    })}
                  </span>
                </span>
                <span className="flex items-center gap-1">
                  <Hash className="size-3 text-subtle" />
                  <span>{t("sessionHistory.turnCount", { count: selectedAttempt.turnCount })}</span>
                </span>
                <span className="flex items-center gap-1">
                  <Cpu className="size-3 text-subtle" />
                  <span>
                    {t("sessionHistory.contextTokens", {
                      count: selectedAttempt.contextTokens.toLocaleString(),
                    })}
                  </span>
                </span>
                {selectedAttempt.workdir && (
                  <span className="flex items-center gap-1 truncate max-w-sm" title={selectedAttempt.workdir}>
                    <FolderOpen className="size-3 text-subtle shrink-0" />
                    <span className="truncate">{selectedAttempt.workdir}</span>
                  </span>
                )}
              </div>
            </div>

            {(readError || live?.error) && <p role="alert" className="px-5 py-2 text-xs text-amber-400">{t(live?.error ? "sessionHistory.persistenceError" : "sessionHistory.readError")}</p>}
            {/* Logs transcript viewer */}
            <div ref={transcriptRef} onScroll={(event) => {
              const element = event.currentTarget;
              const atBottom = element.scrollHeight - element.scrollTop - element.clientHeight <= 32;
              followLatestRef.current = atBottom;
              setShowLatest(!atBottom && visibleLogs.length > 0);
            }} className="relative flex-1 overflow-y-auto p-5 [overflow-anchor:none]">
              <div className="space-y-4">
              {loadingLogs && !live ? (
                <div className="flex items-center justify-center p-12 text-xs text-muted gap-2">
                  <RefreshCw className="size-4 animate-spin text-accent" />
                  <span>{t("sessionHistory.loadingLogs")}</span>
                </div>
              ) : visibleLogs.length === 0 ? (
                <div className="flex flex-col items-center justify-center p-12 text-center text-xs text-muted">
                  <MessageSquare className="size-8 text-subtle mb-2" />
                  <span>{t(selectedAttempt.taskId != null ? "sessionHistory.noTaskMessages" : "sessionHistory.noMessages")}</span>
                </div>
              ) : (
                visibleLogs.map((log) => {
                  const isUser = log.role === "USER";
                  const isAssistant = log.role === "ASSISTANT";

                  // If content is empty, parse meta_json to retrieve text/parts
                  let displayContent = log.content || "";
                  let metaObj: any = null;
                  try {
                    if (log.metaJson) {
                      metaObj = JSON.parse(log.metaJson);
                    }
                  } catch {
                    // Ignore JSON parse errors
                  }

                  if (!displayContent && metaObj) {
                    if (typeof metaObj.text === "string" && metaObj.text) {
                      displayContent = metaObj.text;
                    } else if (Array.isArray(metaObj.parts)) {
                      const textParts = metaObj.parts
                        .filter((p: any) => p && (p.type === "text" || p.type === "reasoning") && typeof p.text === "string")
                        .map((p: any) => p.text);
                      if (textParts.length > 0) {
                        displayContent = textParts.join("\n\n");
                      }
                    }
                  }

                  if (!displayContent && metaObj?.status === "streaming") displayContent = t("sessionHistory.waitingResponse");

                  const hasTools = Array.isArray(metaObj?.parts) && metaObj.parts.some((p: any) => p?.type === "tool");
                  const toolParts = hasTools ? metaObj.parts.filter((p: any) => p?.type === "tool") : [];

                  const parts: any[] = Array.isArray(metaObj?.parts) ? metaObj.parts : [];
                  const reasoningParts = parts.filter((p: any) => p && p.type === "reasoning");
                  const hasProcess = reasoningParts.length > 0 || toolParts.length > 0;

                  return (
                    <div
                      key={log.messageId}
                      data-message-role={log.role}
                      data-message-status={metaObj?.status}
                      className={cn(
                        "rounded-lg border p-4 text-xs space-y-2",
                        isUser
                          ? "border-accent/30 bg-accent/5"
                          : isAssistant
                          ? "border-border bg-elevated/50"
                          : "border-border/60 bg-base/60 font-mono text-subtle",
                      )}
                    >
                      {/* Header of message */}
                      <div className="flex items-center justify-between border-b border-border/40 pb-2">
                        <div className="flex items-center gap-2">
                          {isUser ? (
                            <span className="flex items-center gap-1.5 font-semibold text-accent">
                              <User className="size-3.5" />
                              <span>{t("sessionHistory.roleUser")}</span>
                            </span>
                          ) : isAssistant ? (
                            <span className="flex items-center gap-1.5 font-semibold text-emerald-400">
                              <Bot className="size-3.5" />
                              <span>{t("sessionHistory.roleAssistant")}</span>
                            </span>
                          ) : (
                            <span className="flex items-center gap-1.5 font-semibold text-subtle">
                              <Terminal className="size-3.5" />
                              <span>{t("sessionHistory.roleSystem", { role: log.role })}</span>
                            </span>
                          )}
                          <span className="text-[10px] text-subtle font-mono">
                            Seq: {log.seq} · Turn: {log.turnIndex}
                          </span>
                        </div>

                        <span className="text-[10px] text-subtle font-mono">
                          {log.createdAt ? new Date(log.createdAt).toLocaleTimeString() : ""}
                        </span>
                      </div>

                      {/* Collapsible Process (Reasoning & Tool Steps) - Collapsed by default */}
                      {hasProcess && (
                        <details className="group rounded-md border border-border/50 bg-base/30">
                          <summary className="flex cursor-pointer list-none items-center justify-between px-3 py-1.5 select-none text-muted hover:text-fg hover:bg-hover/20 rounded-md transition-colors">
                            <div className="flex items-center gap-1.5">
                              <Brain className="size-3.5 text-muted shrink-0" />
                              <span className="font-medium text-[11px]">
                                {t("sessionHistory.processSummary", {
                                  prefix:
                                    reasoningParts.length > 0
                                      ? t("sessionHistory.processSummaryDeep")
                                      : "",
                                  count: toolParts.length,
                                })}
                              </span>
                            </div>
                            <span className="text-[10px] text-subtle font-mono group-open:hidden">
                              {t("sessionHistory.clickExpand")}
                            </span>
                            <span className="text-[10px] text-subtle font-mono hidden group-open:inline">
                              {t("sessionHistory.collapse")}
                            </span>
                          </summary>

                          <div className="border-t border-border/30 p-2.5 space-y-2.5 bg-sidebar/40">
                            {/* Reasoning text */}
                            {reasoningParts.map((r: any, idx: number) => (
                              <div key={idx} className="rounded bg-base/40 p-2 text-[11px] leading-relaxed text-muted italic border border-border/30 whitespace-pre-wrap">
                                {r.text}
                              </div>
                            ))}

                            {/* Tools */}
                            {toolParts.length > 0 && (
                              <div className="space-y-1.5">
                                {toolParts.map((tool: any, idx: number) => (
                                  <details key={idx} className="group/tool rounded border border-border/50 bg-base/60 text-[11px] font-mono">
                                    <summary className="flex cursor-pointer list-none items-center justify-between p-2 hover:bg-hover/30 select-none">
                                      <div className="flex items-center gap-1.5 min-w-0">
                                        <Terminal className="size-3 text-muted shrink-0" />
                                        <span className="font-semibold text-fg truncate">
                                          {tool.title || tool.tool || "Tool"}
                                        </span>
                                      </div>
                                      <div className="flex items-center gap-2 shrink-0">
                                        <span>
                                          {tool.ok === false
                                            ? t("sessionHistory.toolFailed")
                                            : tool.ok === true
                                              ? t("sessionHistory.toolOk")
                                              : t("sessionHistory.toolPending")}
                                        </span>
                                        <span className="text-[10px] text-subtle group-open/tool:hidden">▾</span>
                                        <span className="text-[10px] text-subtle hidden group-open/tool:inline">▴</span>
                                      </div>
                                    </summary>
                                    <div className="border-t border-border/30 p-2 bg-base/20 space-y-1">
                                      {tool.detail && (
                                        <div className="text-muted text-[10px] break-all">{tool.detail}</div>
                                      )}
                                      {tool.output && (
                                        <pre className="max-h-40 overflow-auto rounded bg-sidebar p-1.5 text-[10px] text-subtle whitespace-pre-wrap">
                                          {tool.output}
                                        </pre>
                                      )}
                                    </div>
                                  </details>
                                ))}
                              </div>
                            )}
                          </div>
                        </details>
                      )}

                      {/* Content */}
                      <div className="text-fg leading-relaxed">
                        {displayContent ? (
                          isAssistant ? (
                            <Markdown text={displayContent} />
                          ) : (
                            <pre className="whitespace-pre-wrap font-sans text-fg">{displayContent}</pre>
                          )
                        ) : (
                          <div className="text-subtle italic">{t("sessionHistory.emptyBodyItalic")}</div>
                        )}
                      </div>
                    </div>
                  );
                })
              )}
              </div>
              {showLatest && <button type="button" onClick={returnToLatest} className="sticky bottom-3 left-full block rounded-full border border-border bg-elevated px-3 py-1.5 text-xs text-fg shadow" aria-label={t("sessionHistory.returnToLatest")}>{t("sessionHistory.returnToLatest")}</button>}
            </div>
          </>
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center p-8 text-center text-xs text-muted">
            <Bot className="size-10 text-subtle mb-3" />
            <h3 className="text-sm font-medium text-fg">{t("sessionHistory.noneSelectedTitle")}</h3>
            <p className="mt-1 text-xs text-subtle max-w-sm">
              {t("sessionHistory.noneSelectedBody")}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

const ROLE_BADGE_CLASS: Record<string, string> = {
  PLANNER: "rounded bg-sky-500/15 border border-sky-500/30 px-1 py-0.2 text-[9px] font-medium text-sky-400",
  EXECUTOR:
    "rounded bg-emerald-500/15 border border-emerald-500/30 px-1 py-0.2 text-[9px] font-medium text-emerald-400",
  REVIEWER:
    "rounded bg-purple-500/15 border border-purple-500/30 px-1 py-0.2 text-[9px] font-medium text-purple-400",
  TESTER: "rounded bg-amber-500/15 border border-amber-500/30 px-1 py-0.2 text-[9px] font-medium text-amber-400",
};

function RoleBadge({ role }: { role: string }) {
  const { t } = useTranslation();
  const label = t(`sessionHistory.roles.${role}`, { defaultValue: role });
  const className =
    ROLE_BADGE_CLASS[role] ?? "rounded bg-hover px-1 py-0.2 text-[9px] font-medium text-muted";
  return <span className={className}>{label}</span>;
}

function historyProvider(a: AttemptRow): string {
  // Legacy unattributed main-chat rows are not proof of an unknown model call.
  if (a.providerName === "UNKNOWN" && !a.modelId && !a.taskId && !a.cliWindowIdentity) return "TOKAGE_UNATTRIBUTED";
  return a.providerName || "UNKNOWN_CLI";
}

function isNonModelHistory(a: AttemptRow): boolean {
  return ["TOKAGE_INTERNAL", "TOKAGE_UNATTRIBUTED"].includes(historyProvider(a));
}

function formatCliName(id: string): string {
  if (id === "TOKAGE_INTERNAL") return i18n.t("sessionHistory.internalNotices");
  if (id === "TOKAGE_UNATTRIBUTED") return i18n.t("sessionHistory.unattributedMessages");
  if (id === "DEBUG_PROVIDER") return i18n.t("debugProvider.title");
  switch (id) {
    case "CLAUDE_CODE":
      return "Claude Code CLI";
    case "CODEX_CLI":
      return "OpenAI Codex CLI";
    case "COPILOT_CLI":
      return "GitHub Copilot CLI";
    case "CURSOR_CLI":
      return "Cursor CLI";
    case "ANTIGRAVITY_CLI":
      return "Antigravity CLI";
    case "AIDER":
      return "Aider CLI";
    case "OPENCODE":
      return "OpenCode CLI";
    case "REASONIX":
      return "Reasonix CLI";
    default:
      return id;
  }
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    const h = String(d.getHours()).padStart(2, "0");
    const min = String(d.getMinutes()).padStart(2, "0");
    return `${m}-${day} ${h}:${min}`;
  } catch {
    return iso.slice(5, 16);
  }
}
