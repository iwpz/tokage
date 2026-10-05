import { useEffect, useMemo, useState } from "react";
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
} from "lucide-react";
import { cn } from "../../lib/cn";
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
  const [scope, setScope] = useState<"session" | "project">("session");
  const [attempts, setAttempts] = useState<AttemptRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedAttemptId, setSelectedAttemptId] = useState<string | null>(null);
  const [chatLogs, setChatLogs] = useState<ChatLogRow[]>([]);
  const [loadingLogs, setLoadingLogs] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [expandedClis, setExpandedClis] = useState<Record<string, boolean>>({});
  const [expandedModels, setExpandedModels] = useState<Record<string, boolean>>({});

  // Reload attempts based on active scope
  const loadAttempts = async () => {
    setLoading(true);
    try {
      const data =
        scope === "session" && sessionId
          ? await listAttempts(sessionId)
          : await listAttempts(undefined, projectId ?? undefined);
      setAttempts(data);
      if (data.length > 0 && !selectedAttemptId) {
        setSelectedAttemptId(data[0].id);
      }
    } catch (e) {
      console.warn("Failed to list attempts:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadAttempts();
  }, [sessionId, projectId, scope]);

  // Load chat logs when an attempt is selected
  useEffect(() => {
    if (!selectedAttemptId) {
      setChatLogs([]);
      return;
    }
    setLoadingLogs(true);
    getChatLogs(selectedAttemptId)
      .then((logs) => {
        setChatLogs(logs);
      })
      .catch((e) => {
        console.warn("Failed to get chat logs:", e);
        setChatLogs([]);
      })
      .finally(() => {
        setLoadingLogs(false);
      });
  }, [selectedAttemptId]);

  // Group attempts into CLI -> Model -> Attempts two-level tree
  const tree = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    const filtered = attempts.filter((a) => {
      if (!q) return true;
      return (
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
      const cli = a.providerName || "UNKNOWN_CLI";
      const model = a.modelId || "default";
      if (!cliMap[cli]) cliMap[cli] = {};
      if (!cliMap[cli][model]) cliMap[cli][model] = [];
      cliMap[cli][model].push(a);
    }

    return cliMap;
  }, [attempts, searchQuery]);

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

  const isLunaDisguised = (modelId: string) => {
    const lower = modelId.toLowerCase();
    return lower.includes("luna") || lower.includes("gpt-6-luna");
  };

  return (
    <div className="flex flex-1 min-h-0 bg-base overflow-hidden">
      {/* Left Tree & Session Navigation */}
      <div className="flex w-80 shrink-0 flex-col border-r border-border bg-sidebar select-none">
        {/* Top filter and scope switch */}
        <div className="flex flex-col gap-2 border-b border-border p-3 bg-elevated/40">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold text-fg flex items-center gap-1.5">
              <Layers className="size-3.5 text-accent" />
              <span>底层模型真实会话</span>
            </span>
            <div className="flex items-center gap-1">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void loadAttempts()}
                title="刷新真实会话列表"
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
              className={cn(
                "flex-1 rounded py-1 text-[11px] font-medium transition-colors text-center",
                scope === "session"
                  ? "bg-elevated text-fg shadow-xs font-semibold"
                  : "text-muted hover:text-fg",
              )}
            >
              当前会话
            </button>
            <button
              type="button"
              onClick={() => setScope("project")}
              className={cn(
                "flex-1 rounded py-1 text-[11px] font-medium transition-colors text-center",
                scope === "project"
                  ? "bg-elevated text-fg shadow-xs font-semibold"
                  : "text-muted hover:text-fg",
              )}
            >
              本工程全部
            </button>
          </div>

          {/* Search box */}
          <div className="relative">
            <Search className="absolute left-2.5 top-2 size-3 text-subtle" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="搜索 CLI、模型或会话..."
              className="w-full rounded-md border border-border bg-elevated pl-7 pr-2.5 py-1 text-xs text-fg placeholder:text-subtle focus:border-accent focus:outline-hidden"
            />
          </div>
        </div>

        {/* Tree List */}
        <div className="flex-1 overflow-y-auto p-2 space-y-1">
          {attempts.length === 0 ? (
            <div className="flex flex-col items-center justify-center p-6 text-center text-xs text-muted">
              <Bot className="size-8 text-subtle mb-2" />
              <span>暂无底层模型交互记录</span>
              <p className="mt-1 text-[11px] text-subtle max-w-[200px]">
                当在对话或规划中运行模型后，底层的真实交互日志将汇总在此。
              </p>
            </div>
          ) : Object.keys(tree).length === 0 ? (
            <div className="p-4 text-center text-xs text-subtle">未匹配到会话</div>
          ) : (
            Object.entries(tree).map(([cli, models]) => {
              const cliExpanded = expandedClis[cli] ?? true;
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
                  {cliExpanded && (
                    <div className="pl-3 pr-1 pb-1 space-y-1 border-t border-border/30 bg-base/40">
                      {Object.entries(models).map(([model, modelAttempts]) => {
                        const modelKey = `${cli}::${model}`;
                        const modelExpanded = expandedModels[modelKey] ?? true;
                        const isLuna = isLunaDisguised(model);

                        return (
                          <div key={modelKey} className="rounded border border-border/30 bg-elevated/40 mt-1">
                            {/* Model Row */}
                            <button
                              type="button"
                              onClick={() => toggleModel(modelKey)}
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
                                {isLuna && (
                                  <span className="inline-flex items-center rounded bg-amber-500/15 border border-amber-500/30 px-1 py-0.2 text-[9px] font-medium text-amber-400 shrink-0">
                                    替身: Luna
                                  </span>
                                )}
                              </div>
                              <span className="text-[10px] text-muted font-mono shrink-0">
                                {modelAttempts.length}
                              </span>
                            </button>

                            {/* Level 3: Attempts list */}
                            {modelExpanded && (
                              <div className="pl-2 pr-1 pb-1 space-y-0.5">
                                {modelAttempts.map((att) => {
                                  const isSelected = att.id === selectedAttemptId;
                                  const timeStr = att.createdAt
                                    ? formatTime(att.createdAt)
                                    : "未知时间";

                                  return (
                                    <button
                                      key={att.id}
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
                                        <RoleBadge role={att.role} />
                                        <span className="truncate text-[10px] font-mono">
                                          {timeStr}
                                        </span>
                                      </div>
                                      <span className="text-[9px] text-subtle font-mono">
                                        {att.turnCount} 回合
                                      </span>
                                    </button>
                                  );
                                })}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  )}
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
                    <span>{formatCliName(selectedAttempt.providerName)}</span>
                  </span>
                  <span className="text-muted">/</span>
                  <span className="font-mono text-xs font-semibold text-accent">
                    {selectedAttempt.modelId}
                  </span>
                  {isLunaDisguised(selectedAttempt.modelId) && (
                    <Badge variant="warning" size="xs" className="gap-1 font-semibold">
                      <Sparkles className="size-3" />
                      <span>开发替身 (Luna)</span>
                    </Badge>
                  )}
                  <RoleBadge role={selectedAttempt.role} />
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

              {/* Luna Disguise Alert Banner */}
              {isLunaDisguised(selectedAttempt.modelId) && (
                <div className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-1.5 text-xs text-amber-300 flex items-center gap-2">
                  <Sparkles className="size-3.5 shrink-0 text-amber-400" />
                  <span>
                    <strong>开发期替身保护生效：</strong>该会话由底层开发替身模型（Luna）真实承接并执行，已节省高昂模型 Token 开销。
                  </span>
                </div>
              )}

              {/* Meta metrics grid */}
              <div className="flex items-center gap-4 text-xs text-muted flex-wrap">
                <span className="flex items-center gap-1">
                  <Clock className="size-3 text-subtle" />
                  <span>创建时间: {selectedAttempt.createdAt ? new Date(selectedAttempt.createdAt).toLocaleString() : "未知"}</span>
                </span>
                <span className="flex items-center gap-1">
                  <Hash className="size-3 text-subtle" />
                  <span>交互轮次: {selectedAttempt.turnCount}</span>
                </span>
                <span className="flex items-center gap-1">
                  <Cpu className="size-3 text-subtle" />
                  <span>上下文 Token: {selectedAttempt.contextTokens.toLocaleString()}</span>
                </span>
                {selectedAttempt.workdir && (
                  <span className="flex items-center gap-1 truncate max-w-sm" title={selectedAttempt.workdir}>
                    <FolderOpen className="size-3 text-subtle shrink-0" />
                    <span className="truncate">{selectedAttempt.workdir}</span>
                  </span>
                )}
              </div>
            </div>

            {/* Logs transcript viewer */}
            <div className="flex-1 overflow-y-auto p-5 space-y-4">
              {loadingLogs ? (
                <div className="flex items-center justify-center p-12 text-xs text-muted gap-2">
                  <RefreshCw className="size-4 animate-spin text-accent" />
                  <span>正在读取真实会话记录...</span>
                </div>
              ) : chatLogs.length === 0 ? (
                <div className="flex flex-col items-center justify-center p-12 text-center text-xs text-muted">
                  <MessageSquare className="size-8 text-subtle mb-2" />
                  <span>该会话暂无留存的消息文本记录</span>
                </div>
              ) : (
                chatLogs.map((log) => {
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

                  const hasTools = Array.isArray(metaObj?.parts) && metaObj.parts.some((p: any) => p?.type === "tool");
                  const toolParts = hasTools ? metaObj.parts.filter((p: any) => p?.type === "tool") : [];

                  const parts: any[] = Array.isArray(metaObj?.parts) ? metaObj.parts : [];
                  const reasoningParts = parts.filter((p: any) => p && p.type === "reasoning");
                  const hasProcess = reasoningParts.length > 0 || toolParts.length > 0;

                  return (
                    <div
                      key={log.id}
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
                              <span>用户输入 (USER)</span>
                            </span>
                          ) : isAssistant ? (
                            <span className="flex items-center gap-1.5 font-semibold text-emerald-400">
                              <Bot className="size-3.5" />
                              <span>模型回复 (ASSISTANT)</span>
                            </span>
                          ) : (
                            <span className="flex items-center gap-1.5 font-semibold text-subtle">
                              <Terminal className="size-3.5" />
                              <span>系统 / 执行输出 ({log.role})</span>
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
                                思考过程与工具调用（{reasoningParts.length > 0 ? "已深度思考 · " : ""}{toolParts.length} 个步骤）
                              </span>
                            </div>
                            <span className="text-[10px] text-subtle font-mono group-open:hidden">点击展开 ▾</span>
                            <span className="text-[10px] text-subtle font-mono hidden group-open:inline">收起 ▴</span>
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
                                {toolParts.map((t: any, idx: number) => (
                                  <details key={idx} className="group/tool rounded border border-border/50 bg-base/60 text-[11px] font-mono">
                                    <summary className="flex cursor-pointer list-none items-center justify-between p-2 hover:bg-hover/30 select-none">
                                      <div className="flex items-center gap-1.5 min-w-0">
                                        <Terminal className="size-3 text-muted shrink-0" />
                                        <span className="font-semibold text-fg truncate">
                                          {t.title || t.tool || "Tool"}
                                        </span>
                                      </div>
                                      <div className="flex items-center gap-2 shrink-0">
                                        <span>{t.ok === false ? "❌ 失败" : t.ok === true ? "✅ 成功" : "⏳"}</span>
                                        <span className="text-[10px] text-subtle group-open/tool:hidden">▾</span>
                                        <span className="text-[10px] text-subtle hidden group-open/tool:inline">▴</span>
                                      </div>
                                    </summary>
                                    <div className="border-t border-border/30 p-2 bg-base/20 space-y-1">
                                      {t.detail && <div className="text-muted text-[10px] break-all">{t.detail}</div>}
                                      {t.output && (
                                        <pre className="max-h-40 overflow-auto rounded bg-sidebar p-1.5 text-[10px] text-subtle whitespace-pre-wrap">
                                          {t.output}
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
                          <div className="text-subtle italic">（此消息无文本正文）</div>
                        )}
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </>
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center p-8 text-center text-xs text-muted">
            <Bot className="size-10 text-subtle mb-3" />
            <h3 className="text-sm font-medium text-fg">未选中会话</h3>
            <p className="mt-1 text-xs text-subtle max-w-sm">
              从左侧的 CLI / 模型树中选择任意一条底层真实交互，即可查看完整的提示词与响应记录。
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

function RoleBadge({ role }: { role: string }) {
  switch (role) {
    case "PLANNER":
      return (
        <span className="rounded bg-sky-500/15 border border-sky-500/30 px-1 py-0.2 text-[9px] font-medium text-sky-400">
          规划者
        </span>
      );
    case "EXECUTOR":
      return (
        <span className="rounded bg-emerald-500/15 border border-emerald-500/30 px-1 py-0.2 text-[9px] font-medium text-emerald-400">
          执行者
        </span>
      );
    case "REVIEWER":
      return (
        <span className="rounded bg-purple-500/15 border border-purple-500/30 px-1 py-0.2 text-[9px] font-medium text-purple-400">
          审查者
        </span>
      );
    case "TESTER":
      return (
        <span className="rounded bg-amber-500/15 border border-amber-500/30 px-1 py-0.2 text-[9px] font-medium text-amber-400">
          测试者
        </span>
      );
    default:
      return (
        <span className="rounded bg-hover px-1 py-0.2 text-[9px] font-medium text-muted">
          {role}
        </span>
      );
  }
}

function formatCliName(id: string): string {
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
