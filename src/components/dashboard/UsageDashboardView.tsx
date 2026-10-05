import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  Calendar,
  CheckCircle2,
  ChevronRight,
  Info,
  Layers,
  PiggyBank,
  RefreshCw,
  ShieldAlert,
  Sparkles,
} from "lucide-react";
import { getUsageDashboard, type UsageDashboard, type ProviderId } from "../../lib/ipc";
import { useUi } from "../../stores/ui";
import { cn } from "../../lib/cn";
import { Badge, Card, IconButton } from "../ui";
import { ProviderLogo } from "../ProviderLogo";

export function UsageDashboardView() {
  const { t } = useTranslation();
  const { closeDashboard } = useUi();
  const [range, setRange] = useState<"today" | "7d" | "30d">("7d");
  const [data, setData] = useState<UsageDashboard | null>(null);
  const [loading, setLoading] = useState(false);
  const [showSavingsDetail, setShowSavingsDetail] = useState(false);

  const loadData = async (r: "today" | "7d" | "30d") => {
    setLoading(true);
    try {
      const res = await getUsageDashboard(r);
      setData(res);
    } catch (err) {
      console.error("Failed to load usage dashboard:", err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadData(range);
  }, [range]);

  const costSplit = data?.costSplit ?? { planningUsd: 0, executionUsd: 0, repairUsd: 0 };
  const planningUsd = costSplit.planningUsd ?? 0;
  const executionUsd = costSplit.executionUsd ?? 0;
  const repairUsd = costSplit.repairUsd ?? 0;
  const totalCost = planningUsd + executionUsd + repairUsd;

  return (
    <div className="flex h-full flex-col overflow-y-auto bg-bg text-fg">
      {/* Sticky Header */}
      <div className="sticky top-0 z-20 flex items-center justify-between border-b border-border bg-bg/95 px-6 py-3.5 backdrop-blur">
        <div className="flex items-center gap-3">
          <IconButton label={t("dashboard.back", "返回工作区")} onClick={closeDashboard}>
            <ArrowLeft className="size-4" />
          </IconButton>
          <div>
            <h1 className="text-base font-semibold leading-none">{t("dashboard.title", "用量与质量看板")}</h1>
            <p className="mt-1 text-xs text-subtle">
              {t("dashboard.subtitle", "额度透视 · 反事实节省账本 · 领域质量矩阵 · Token 构成")}
            </p>
          </div>
        </div>

        {/* Range Selector & Refresh */}
        <div className="flex items-center gap-2">
          <div className="flex rounded-lg border border-border bg-sidebar p-0.5 text-xs">
            {(["today", "7d", "30d"] as const).map((r) => (
              <button
                key={r}
                type="button"
                onClick={() => setRange(r)}
                className={cn(
                  "rounded-md px-3 py-1 font-medium transition-colors",
                  range === r ? "bg-elevated text-fg shadow-sm" : "text-subtle hover:text-fg"
                )}
              >
                {t(`dashboard.ranges.${r}`, r === "today" ? "今日" : r === "7d" ? "近7天" : "近30天")}
              </button>
            ))}
          </div>

          <IconButton
            label={t("dashboard.refresh", "刷新")}
            onClick={() => void loadData(range)}
            className={cn("size-8", loading && "animate-spin text-accent")}
          >
            <RefreshCw className="size-3.5" />
          </IconButton>
        </div>
      </div>

      <div className="mx-auto flex w-full max-w-6xl flex-col gap-6 p-6">
        {/* Top 3 Summary Cards */}
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
          {/* 1. Savings Card */}
          <Card className="relative flex flex-col justify-between overflow-hidden p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium text-subtle">{t("dashboard.savings.title", "反事实节省账本")}</span>
                <span className="rounded-full bg-accent/15 p-1.5 text-accent">
                  <PiggyBank className="size-4" />
                </span>
              </div>
              <div className="mt-3 flex items-baseline gap-2">
                <span className="text-3xl font-bold tracking-tight text-accent">
                  ${(data?.savings?.totalUsd ?? 0).toFixed(2)}
                </span>
                <span className="text-xs text-subtle">
                  {t("dashboard.savings.netSaved", "累计节省")}
                </span>
              </div>

              <div className="mt-4 flex flex-col gap-1.5 border-t border-border/60 pt-3 text-xs">
                <div className="flex justify-between text-subtle">
                  <span>{t("dashboard.savings.assignment", "任务全局分流 (ASSIGNMENT)")}</span>
                  <span className="font-mono text-fg">${(data?.savings?.byKind?.ASSIGNMENT ?? 0).toFixed(2)}</span>
                </div>
                <div className="flex justify-between text-subtle">
                  <span>{t("dashboard.savings.context", "上下文复用与压缩 (CONTEXT)")}</span>
                  <span className="font-mono text-fg">${(data?.savings?.byKind?.CONTEXT ?? 0).toFixed(2)}</span>
                </div>
                <div className="flex justify-between text-subtle">
                  <span>{t("dashboard.savings.direct", "直接分发 (DIRECT)")}</span>
                  <span className="font-mono text-fg">${(data?.savings?.byKind?.DIRECT ?? 0).toFixed(2)}</span>
                </div>
              </div>
            </div>

            <div>
              <button
                type="button"
                onClick={() => setShowSavingsDetail(!showSavingsDetail)}
                className="mt-3 flex w-full items-center justify-between text-[11px] text-muted hover:text-fg"
              >
                <span className="flex items-center gap-1">
                  <Info className="size-3 text-subtle" />
                  <span>{t("dashboard.savings.refModel", "参考模型")}: {data?.savings?.referenceModel ?? "claude-3-7-sonnet"}</span>
                </span>
                <ChevronRight className={cn("size-3 transition-transform", showSavingsDetail && "rotate-90")} />
              </button>

              {showSavingsDetail && (
                <p className="mt-2 rounded bg-sidebar p-2 text-[10px] leading-relaxed text-subtle">
                  {t("dashboard.savings.disclaimer", "基准为反事实估算：如果所有执行均由最强参考模型包办的预估成本。Tokage 实际成本 = 规划 + 执行 + 修复。节省 = 基准 − 实际。")}
                </p>
              )}
            </div>
          </Card>

          {/* 2. Quality Card */}
          <Card className="flex flex-col justify-between p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium text-subtle">{t("dashboard.quality.title", "编排一次通过率")}</span>
                <span className="rounded-full bg-emerald-500/15 p-1.5 text-emerald-500">
                  <CheckCircle2 className="size-4" />
                </span>
              </div>
              <div className="mt-3 flex items-baseline gap-2">
                <span className="text-3xl font-bold tracking-tight text-emerald-500">
                  {Math.round((data?.quality?.firstPassRate ?? 1.0) * 100)}%
                </span>
                <span className="text-xs text-subtle">
                  {t("dashboard.quality.totalTasks", "全部验收关卡")}
                </span>
              </div>
            </div>

            <p className="mt-4 border-t border-border/60 pt-3 text-xs leading-relaxed text-muted">
              {t("dashboard.quality.desc", "统计模型首次交付即通过本地关卡（Cargo/Test/Lint/Scope）的比例。未通过者触发同窗修复或领域改派。")}
            </p>
          </Card>

          {/* 3. Cost Split Card */}
          <Card className="flex flex-col justify-between p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium text-subtle">{t("dashboard.costSplit.title", "阶段成本构成")}</span>
                <span className="rounded-full bg-blue-500/15 p-1.5 text-blue-500">
                  <Layers className="size-4" />
                </span>
              </div>
              <div className="mt-3 flex items-baseline gap-2">
                <span className="text-3xl font-bold tracking-tight">
                  ${totalCost.toFixed(2)}
                </span>
                <span className="text-xs text-subtle">
                  {t("dashboard.costSplit.totalSpent", "实际总支出")}
                </span>
              </div>
            </div>

            <div>
              {/* Proportion Bar */}
              <div className="mt-4 flex h-2 w-full overflow-hidden rounded-full bg-border">
                <div
                  style={{ width: totalCost > 0 ? `${(planningUsd / totalCost) * 100}%` : "0%" }}
                  className="bg-amber-500 transition-all"
                  title={`规划: $${planningUsd.toFixed(2)}`}
                />
                <div
                  style={{ width: totalCost > 0 ? `${(executionUsd / totalCost) * 100}%` : "100%" }}
                  className="bg-accent transition-all"
                  title={`执行: $${executionUsd.toFixed(2)}`}
                />
                <div
                  style={{ width: totalCost > 0 ? `${(repairUsd / totalCost) * 100}%` : "0%" }}
                  className="bg-rose-500 transition-all"
                  title={`修复: $${repairUsd.toFixed(2)}`}
                />
              </div>

              <div className="mt-3 flex justify-between text-[11px] text-muted">
                <span className="flex items-center gap-1.5">
                  <span className="size-2 rounded-full bg-amber-500" />
                  <span>规划: ${planningUsd.toFixed(2)}</span>
                </span>
                <span className="flex items-center gap-1.5">
                  <span className="size-2 rounded-full bg-accent" />
                  <span>执行: ${executionUsd.toFixed(2)}</span>
                </span>
                <span className="flex items-center gap-1.5">
                  <span className="size-2 rounded-full bg-rose-500" />
                  <span>修复: ${repairUsd.toFixed(2)}</span>
                </span>
              </div>
            </div>
          </Card>
        </div>

        {/* Provider Quota Rings (P8.m3c) */}
        <div>
          <div className="mb-3 flex items-center justify-between">
            <h2 className="text-sm font-semibold tracking-tight">{t("dashboard.providers.title", "Provider 额度与健康状态")}</h2>
            <span className="text-xs text-subtle">{t("dashboard.providers.desc", "订阅滚动额度 · 缓存命中率 · 熔断监督")}</span>
          </div>

          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {data?.providers?.map((p) => (
              <ProviderCard key={p.provider} provider={p} />
            ))}
          </div>
        </div>

        {/* Quality Matrix (Model x Domain) */}
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <Card className="p-5">
            <div className="mb-3 flex items-center justify-between">
              <div>
                <h3 className="text-sm font-semibold">{t("dashboard.matrix.title", "首次通过率矩阵 (Model × Domain)")}</h3>
                <p className="mt-0.5 text-xs text-subtle">{t("dashboard.matrix.subtitle", "各模型在特定领域的实测稳定度")}</p>
              </div>
              <Badge variant="muted" className="text-xs">
                {data?.quality?.matrix?.length ?? 0} {t("dashboard.matrix.domains", "项配置")}
              </Badge>
            </div>

            {(!data?.quality?.matrix || data.quality.matrix.length === 0) ? (
              <p className="py-8 text-center text-xs text-subtle">{t("dashboard.matrix.empty", "暂无任务关卡样本")}</p>
            ) : (
              <div className="divide-y divide-border/60 overflow-hidden rounded-lg border border-border">
                {data.quality.matrix.slice(0, 8).map((m, idx) => (
                  <div key={idx} className="flex items-center justify-between p-3 text-xs hover:bg-hover/50">
                    <div className="flex flex-col gap-0.5">
                      <span className="font-medium text-fg">{m.model}</span>
                      <span className="text-[11px] font-mono text-subtle">{m.domain}</span>
                    </div>
                    <div className="flex items-center gap-3">
                      <span className="text-subtle font-mono text-[11px]">{m.tasks} 任务</span>
                      <span
                        className={cn(
                          "rounded-full px-2 py-0.5 font-mono text-xs font-semibold",
                          (m.firstPassRate ?? 1) >= 0.9
                            ? "bg-emerald-500/15 text-emerald-500"
                            : (m.firstPassRate ?? 1) >= 0.7
                            ? "bg-amber-500/15 text-amber-500"
                            : "bg-rose-500/15 text-rose-500"
                        )}
                      >
                        {Math.round((m.firstPassRate ?? 1) * 100)}%
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </Card>

          {/* Token Area / Stacked Chart (P8.m3c) */}
          <Card className="p-5">
            <div className="mb-3 flex items-center justify-between">
              <div>
                <h3 className="text-sm font-semibold">{t("dashboard.tokens.title", "Token 消耗与缓存吞吐")}</h3>
                <p className="mt-0.5 text-xs text-subtle">{t("dashboard.tokens.subtitle", "未缓存输入 / 缓存读 / 缓存写 / 输出")}</p>
              </div>
              <span className="flex items-center gap-1.5 text-xs text-accent">
                <Sparkles className="size-3.5" />
                <span>Prompt Caching</span>
              </span>
            </div>

            <TokenSeriesChart points={data?.tokenSeries ?? []} />
          </Card>
        </div>
      </div>
    </div>
  );
}

function ProviderCard({ provider }: { provider: UsageDashboard["providers"][number] }) {
  const isMelted = provider.status === "MELTED";
  const hasPct = typeof provider.usagePct === "number";
  const pct = hasPct ? provider.usagePct! : 0;
  const pctDisplay = hasPct ? `${Math.round(pct * 100)}%` : "—";

  // Arc calculation for quota ring (radius 28, circ ~ 176)
  const radius = 28;
  const circumference = 2 * Math.PI * radius;
  const strokeDashoffset = circumference - (pct * circumference);

  const ringColor =
    pct >= 0.95
      ? "text-rose-500 stroke-rose-500"
      : pct >= 0.8
      ? "text-amber-500 stroke-amber-500"
      : "text-accent stroke-accent";

  const resetTimeStr = formatResetTime(provider.resetAt);

  return (
    <Card className={cn("relative flex flex-col justify-between p-4 transition-all", isMelted && "border-rose-500/50 bg-rose-500/5")}>
      <div>
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-2">
            <ProviderLogo id={provider.provider as ProviderId} name={providerLabel(provider.provider)} className="size-5" />
            <span className="font-semibold text-xs tracking-tight">{providerLabel(provider.provider)}</span>
          </div>
          <Badge
            variant={provider.billing === "SUBSCRIPTION" ? "default" : "muted"}
            className="text-[10px] font-mono"
          >
            {provider.billing === "SUBSCRIPTION" ? "订阅" : "按量"}
          </Badge>
        </div>

        {/* Quota Ring */}
        <div className="my-3 flex items-center justify-center gap-4">
          <div className="relative flex size-20 items-center justify-center">
            <svg className="size-full -rotate-90" viewBox="0 0 72 72">
              <circle
                cx="36"
                cy="36"
                r={radius}
                className="stroke-border"
                strokeWidth="6"
                fill="transparent"
              />
              {hasPct && (
                <circle
                  cx="36"
                  cy="36"
                  r={radius}
                  className={cn("transition-all duration-500", ringColor)}
                  strokeWidth="6"
                  strokeDasharray={circumference}
                  strokeDashoffset={strokeDashoffset}
                  strokeLinecap="round"
                  fill="transparent"
                />
              )}
            </svg>
            <div className="absolute flex flex-col items-center justify-center text-center">
              <span className="text-sm font-bold tracking-tight">{pctDisplay}</span>
              {provider.usagePctSource && (
                <span className="text-[9px] uppercase tracking-wider text-subtle">
                  {provider.usagePctSource === "SNAPSHOT" ? "快照" : "推算"}
                </span>
              )}
            </div>
          </div>

          <div className="flex flex-col gap-1 text-xs">
            <div className="flex items-center gap-1.5">
              <span className="text-subtle">缓存命中:</span>
              <span className="font-mono font-medium text-fg">{provider.cacheHitRate ?? 0}%</span>
            </div>
            {typeof provider.spentUsd === "number" && (
              <div className="flex items-center gap-1.5">
                <span className="text-subtle">支出:</span>
                <span className="font-mono font-medium text-fg">${provider.spentUsd.toFixed(2)}</span>
              </div>
            )}
            {typeof provider.equivCostUsd === "number" && (
              <div className="flex items-center gap-1.5">
                <span className="text-subtle">等价用量:</span>
                <span className="font-mono font-medium text-fg">${provider.equivCostUsd.toFixed(2)}</span>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Melt Alert or Reset Timer */}
      <div className="border-t border-border/50 pt-2.5">
        {isMelted ? (
          <div className="flex items-center gap-1.5 rounded bg-rose-500/10 px-2 py-1 text-[11px] font-medium text-rose-500">
            <ShieldAlert className="size-3.5 shrink-0" />
            <span className="truncate">熔断: {provider.meltReason ?? "QUOTA_HARD"}</span>
          </div>
        ) : resetTimeStr ? (
          <div className="flex items-center gap-1 text-[10px] text-subtle">
            <Calendar className="size-3" />
            <span>重置时间: {resetTimeStr}</span>
          </div>
        ) : (
          <div className="flex items-center gap-1.5 text-[10px] text-emerald-500">
            <span className="size-1.5 rounded-full bg-emerald-500" />
            <span>运行状态良好</span>
          </div>
        )}
      </div>
    </Card>
  );
}

function formatResetTime(iso?: string | null): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (isNaN(d.getTime())) return null;
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function TokenSeriesChart({ points }: { points: UsageDashboard["tokenSeries"] }) {
  const { t } = useTranslation();
  if (points.length === 0) {
    return <div className="py-12 text-center text-xs text-subtle">{t("dashboard.tokens.empty", "当前时间范围内暂无 Token 事件")}</div>;
  }

  // Aggregate by bucket
  const bucketsMap: Record<string, { input: number; cacheRead: number; cacheWrite: number; output: number }> = {};
  for (const pt of points) {
    if (!bucketsMap[pt.bucket]) {
      bucketsMap[pt.bucket] = { input: 0, cacheRead: 0, cacheWrite: 0, output: 0 };
    }
    bucketsMap[pt.bucket].input += pt.input;
    bucketsMap[pt.bucket].cacheRead += pt.cacheRead;
    bucketsMap[pt.bucket].cacheWrite += pt.cacheWrite;
    bucketsMap[pt.bucket].output += pt.output;
  }

  const entries = Object.entries(bucketsMap).slice(-7);
  const maxTotal = Math.max(
    1,
    ...entries.map(([, v]) => v.input + v.cacheRead + v.cacheWrite + v.output)
  );

  return (
    <div className="mt-4 flex flex-col gap-3">
      <div className="flex h-40 items-end gap-3 border-b border-border pb-2 pt-4">
        {entries.map(([bucket, val]) => {
          const total = val.input + val.cacheRead + val.cacheWrite + val.output;
          const heightPct = Math.max(5, (total / maxTotal) * 100);

          return (
            <div key={bucket} className="group relative flex flex-1 flex-col items-center justify-end h-full">
              <div
                style={{ height: `${heightPct}%` }}
                className="flex w-full flex-col-reverse overflow-hidden rounded-t bg-border/40 transition-all hover:brightness-110"
              >
                {/* Stacked bars: Output, CacheWrite, CacheRead, Input */}
                <div
                  style={{ height: total > 0 ? `${(val.output / total) * 100}%` : "0%" }}
                  className="bg-amber-500"
                  title={`Output: ${(val.output / 1000).toFixed(1)}k`}
                />
                <div
                  style={{ height: total > 0 ? `${(val.cacheWrite / total) * 100}%` : "0%" }}
                  className="bg-purple-500"
                  title={`Cache Write: ${(val.cacheWrite / 1000).toFixed(1)}k`}
                />
                <div
                  style={{ height: total > 0 ? `${(val.cacheRead / total) * 100}%` : "0%" }}
                  className="bg-accent"
                  title={`Cache Read: ${(val.cacheRead / 1000).toFixed(1)}k`}
                />
                <div
                  style={{ height: total > 0 ? `${(val.input / total) * 100}%` : "0%" }}
                  className="bg-blue-400"
                  title={`Uncached Input: ${(val.input / 1000).toFixed(1)}k`}
                />
              </div>
              <span className="mt-2 text-[10px] font-mono text-subtle">{bucket}</span>
            </div>
          );
        })}
      </div>

      {/* Legend */}
      <div className="flex flex-wrap items-center justify-center gap-4 text-[11px] text-muted">
        <span className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm bg-blue-400" />
          <span>未缓存输入</span>
        </span>
        <span className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm bg-accent" />
          <span>缓存读 (Cache Read)</span>
        </span>
        <span className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm bg-purple-500" />
          <span>缓存写 (Cache Write)</span>
        </span>
        <span className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm bg-amber-500" />
          <span>输出 (Output)</span>
        </span>
      </div>
    </div>
  );
}

function providerLabel(id: ProviderId | string): string {
  switch (id) {
    case "CLAUDE_CODE":
      return "Claude Code";
    case "CODEX_CLI":
      return "Codex CLI";
    case "CURSOR_CLI":
      return "Cursor CLI";
    case "COPILOT_CLI":
      return "GitHub Copilot";
    case "ANTIGRAVITY_CLI":
      return "Antigravity";
    case "AIDER":
      return "Aider";
    case "OPENCODE":
      return "OpenCode";
    default:
      return id;
  }
}
