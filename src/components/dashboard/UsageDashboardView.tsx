import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
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
  BarChart3,
  HelpCircle,
  Cpu,
} from "lucide-react";
import {
  getUsageDashboard,
  type UsageDashboard,
  type ProviderId,
  type DailyUsagePoint,
  type DashboardProviderInfo,
  type DashboardModelUsage,
} from "../../lib/ipc";
import { useUi } from "../../stores/ui";
import { cn } from "../../lib/cn";
import { Badge, Card, IconButton } from "../ui";
import { ProviderLogo } from "../ProviderLogo";

export interface TooltipState {
  rect: DOMRect;
  content: React.ReactNode;
  width?: number;
}

interface DashboardTooltipContextValue {
  showTooltip: (rect: DOMRect, content: React.ReactNode, width?: number) => void;
  hideTooltip: () => void;
}

const DashboardTooltipContext = createContext<DashboardTooltipContextValue>({
  showTooltip: () => {},
  hideTooltip: () => {},
});

export function useDashboardTooltip() {
  return useContext(DashboardTooltipContext);
}

function FloatingPortalTooltip({ tooltip }: { tooltip: TooltipState | null }) {
  if (!tooltip || typeof document === "undefined" || !document.body) return null;

  const width = tooltip.width ?? 260;
  const rect = tooltip.rect;

  // Margin from window edge so tooltip is never clipped
  const margin = 12;
  let left = rect.left + rect.width / 2 - width / 2;
  const maxLeft = Math.max(margin, window.innerWidth - width - margin);
  left = Math.max(margin, Math.min(maxLeft, left));

  // Determine top/bottom position: default placed above the target element
  // If target is too close to top (< 180px from window top), flip below
  const estimatedHeight = 175;
  let top = rect.top - estimatedHeight - 8;
  if (top < margin) {
    top = rect.bottom + 8;
  }

  return createPortal(
    <div
      style={{
        left: `${left}px`,
        top: `${top}px`,
        width: `${width}px`,
        backgroundColor: "var(--bg-elevated)",
        color: "var(--fg)",
      }}
      className="pointer-events-none fixed z-[99999] rounded-lg border border-border p-3 text-xs shadow-2xl backdrop-blur-none select-none transition-opacity duration-75 animate-in fade-in"
    >
      {tooltip.content}
    </div>,
    document.body
  );
}

export function UsageDashboardView() {
  const { t } = useTranslation();
  const { closeDashboard } = useUi();
  const [range, setRange] = useState<"today" | "7d" | "30d">("7d");
  const [data, setData] = useState<UsageDashboard | null>(null);
  const [loading, setLoading] = useState(false);
  const [showSavingsDetail, setShowSavingsDetail] = useState(false);
  const [activeTooltip, setActiveTooltip] = useState<TooltipState | null>(null);

  const showTooltip = useCallback((rect: DOMRect, content: React.ReactNode, width = 260) => {
    setActiveTooltip({ rect, content, width });
  }, []);

  const hideTooltip = useCallback(() => {
    setActiveTooltip(null);
  }, []);

  // Dismiss tooltip on window scroll
  useEffect(() => {
    const onScroll = () => setActiveTooltip(null);
    window.addEventListener("scroll", onScroll, { capture: true, passive: true });
    return () => window.removeEventListener("scroll", onScroll, { capture: true });
  }, []);

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
    <DashboardTooltipContext.Provider value={{ showTooltip, hideTooltip }}>
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

        {/* 4. Daily Activity Heatmap (GitHub Style) */}
        <DailyActivityHeatmap points={data?.dailyUsage ?? []} />

        {/* 5. Provider Quota & Status */}
        <div>
          <div className="mb-3 flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex items-center gap-2">
              <h2 className="text-sm font-semibold tracking-tight">{t("dashboard.providers.title", "Provider 额度与健康状态")}</h2>
              <div
                className="flex items-center text-subtle hover:text-fg cursor-help transition-colors"
                onMouseEnter={(e) =>
                  showTooltip(
                    e.currentTarget.getBoundingClientRect(),
                    <div className="text-[11px] leading-relaxed text-fg">
                      {t("dashboard.providers.trackableNotice", "仅展示可统计 Token 的 Provider（例如 GitHub Copilot 仅按 Premium 请求计费，不计入 Token 消耗统计）")}
                    </div>,
                    280
                  )
                }
                onMouseLeave={hideTooltip}
              >
                <HelpCircle className="size-3.5" />
              </div>
            </div>
            <span className="text-xs text-subtle">
              {t("dashboard.providers.trackableNotice", "仅展示可统计 Token 的 Provider")}
            </span>
          </div>

          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {data?.providers?.map((p) => (
              <ProviderCard key={p.provider} provider={p} />
            ))}
          </div>
        </div>

        {/* 6. Provider Model Detailed Breakdown Bar Chart */}
        <ProviderModelBarChart providers={data?.providers ?? []} />

        {/* 7. Quality Matrix (Model x Domain) & Token Series */}
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          {/* Quality Matrix */}
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
    <FloatingPortalTooltip tooltip={activeTooltip} />
  </DashboardTooltipContext.Provider>
);
}

/**
 * 4. Daily Activity Heatmap: GitHub-style multi-row calendar mode.
 * 7 rows (Sunday to Saturday), 52 week columns, small squares.
 * Color vibrancy corresponds to total token volume; grey for 0/no data.
 * Tooltip uses portal so it is never clipped by card or application boundaries.
 */
function DailyActivityHeatmap({ points }: { points: DailyUsagePoint[] }) {
  const { t } = useTranslation();
  const { showTooltip, hideTooltip } = useDashboardTooltip();
  const scrollRef = useRef<HTMLDivElement>(null);

  const { weeks, totalTokens, activeDays, maxTokens } = useMemo(() => {
    const pointsMap = new Map<string, DailyUsagePoint>();
    for (const pt of points) {
      pointsMap.set(pt.date, pt);
    }

    const today = new Date();
    const todayYear = today.getFullYear();
    const todayMonth = today.getMonth();
    const todayDate = today.getDate();
    const todayLocal = new Date(todayYear, todayMonth, todayDate);
    const pad = (n: number) => (n < 10 ? `0${n}` : `${n}`);
    const todayStr = `${todayYear}-${pad(todayMonth + 1)}-${pad(todayDate)}`;
    const todayDayOfWeek = todayLocal.getDay(); // 0 is Sun, 6 is Sat

    const weeksCount = 52;
    // Current week Sunday
    const currentWeekSunday = new Date(todayYear, todayMonth, todayDate - todayDayOfWeek);
    // Grid start Sunday
    const gridStartSunday = new Date(currentWeekSunday);
    gridStartSunday.setDate(currentWeekSunday.getDate() - (weeksCount - 1) * 7);

    const cols: Array<{
      weekIndex: number;
      monthLabel?: string;
      days: Array<{
        date: string;
        dayOfWeek: number;
        data: DailyUsagePoint | null;
        isToday: boolean;
        isFuture: boolean;
      }>;
    }> = [];

    let lastLabeledMonth = -1;
    let weeksSinceLastMonth = 99;
    let sumTokens = 0;
    let activeCount = 0;
    let max = 1;

    for (let w = 0; w < weeksCount; w++) {
      const days: Array<{
        date: string;
        dayOfWeek: number;
        data: DailyUsagePoint | null;
        isToday: boolean;
        isFuture: boolean;
      }> = [];
      let monthLabel: string | undefined;

      for (let d = 0; d < 7; d++) {
        const cellDate = new Date(gridStartSunday);
        cellDate.setDate(gridStartSunday.getDate() + w * 7 + d);
        const y = cellDate.getFullYear();
        const m = cellDate.getMonth();
        const dt = cellDate.getDate();
        const dateStr = `${y}-${pad(m + 1)}-${pad(dt)}`;
        const isFuture = cellDate.getTime() > todayLocal.getTime();
        const isToday = dateStr === todayStr;
        const dayData = !isFuture ? (pointsMap.get(dateStr) ?? null) : null;

        if (dayData && dayData.totalTokens > 0) {
          sumTokens += dayData.totalTokens;
          activeCount += 1;
          if (dayData.totalTokens > max) {
            max = dayData.totalTokens;
          }
        }

        days.push({
          date: dateStr,
          dayOfWeek: d,
          data: dayData,
          isToday,
          isFuture,
        });

        // Show month label when the month changes on Sunday/Monday
        if (d === 0) {
          if (m !== lastLabeledMonth && weeksSinceLastMonth >= 2) {
            monthLabel = cellDate.toLocaleDateString(undefined, { month: "short" });
            lastLabeledMonth = m;
            weeksSinceLastMonth = 0;
          } else {
            weeksSinceLastMonth++;
          }
        }
      }

      cols.push({
        weekIndex: w,
        monthLabel,
        days,
      });
    }

    return {
      weeks: cols,
      totalTokens: sumTokens,
      activeDays: activeCount,
      maxTokens: max,
    };
  }, [points]);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollLeft = scrollRef.current.scrollWidth;
    }
  }, [points]);

  const getCellClass = (tokens: number) => {
    if (tokens <= 0) {
      return "border border-border/80 dark:border-border/60 bg-transparent hover:border-fg/50 hover:bg-hover/20";
    }
    const ratio = tokens / maxTokens;
    if (ratio <= 0.25) {
      return "bg-emerald-200 dark:bg-emerald-950/90 border border-emerald-300 dark:border-emerald-800/50 hover:ring-2 hover:ring-emerald-400";
    }
    if (ratio <= 0.5) {
      return "bg-emerald-400 dark:bg-emerald-800 border border-emerald-500/60 dark:border-emerald-700/60 hover:ring-2 hover:ring-emerald-400";
    }
    if (ratio <= 0.75) {
      return "bg-emerald-500 dark:bg-emerald-600 hover:ring-2 hover:ring-emerald-300";
    }
    return "bg-emerald-600 dark:bg-emerald-400 shadow-sm shadow-emerald-500/30 hover:ring-2 hover:ring-emerald-300";
  };

  return (
    <Card className="p-5">
      <div className="mb-4 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="flex items-center gap-2">
            <Calendar className="size-4 text-accent" />
            <h3 className="text-sm font-semibold">{t("dashboard.heatmap.title", "每日用量活跃热力图")}</h3>
          </div>
          <p className="mt-0.5 text-xs text-subtle">
            {t("dashboard.heatmap.subtitle", "年度 Token 消耗活跃度日历（颜色越鲜艳消耗量越大，空心方块无数据）")}
          </p>
        </div>

        {/* Summary Badges */}
        <div className="flex items-center gap-3 text-xs">
          <div className="flex items-baseline gap-1 font-mono">
            <span className="text-subtle">{t("dashboard.heatmap.activeDays", "活跃天数")}:</span>
            <span className="font-semibold text-accent">{activeDays}</span>
            <span className="text-subtle">/ 365</span>
          </div>
          <div className="flex items-baseline gap-1 font-mono">
            <span className="text-subtle">{t("dashboard.heatmap.yearTotal", "过去 1 年累计消耗")}:</span>
            <span className="font-semibold text-fg">
              {totalTokens >= 1_000_000
                ? `${(totalTokens / 1_000_000).toFixed(2)}M`
                : `${(totalTokens / 1_000).toFixed(1)}k`}
            </span>
          </div>
        </div>
      </div>

      {/* Calendar Grid Container with Scrollable Row */}
      <div className="overflow-x-auto pb-2 pt-1" ref={scrollRef}>
        <div className="inline-flex flex-col min-w-max">
          {/* Month labels row */}
          <div className="flex pl-6 gap-[3px] h-4 mb-1">
            {weeks.map((w, idx) => (
              <div key={idx} className="w-[11px] text-[9px] font-mono text-subtle select-none whitespace-nowrap">
                {w.monthLabel ?? ""}
              </div>
            ))}
          </div>

          {/* Weekday labels + grid */}
          <div className="flex gap-1.5 items-start">
            {/* Weekday column (Mon, Wed, Fri aligned with rows 1, 3, 5) */}
            <div className="flex flex-col gap-[3px] select-none text-right pr-1">
              <span className="h-[11px] text-[9px] leading-[11px] text-transparent">日</span>
              <span className="h-[11px] text-[9px] leading-[11px] text-subtle font-mono">一</span>
              <span className="h-[11px] text-[9px] leading-[11px] text-transparent">二</span>
              <span className="h-[11px] text-[9px] leading-[11px] text-subtle font-mono">三</span>
              <span className="h-[11px] text-[9px] leading-[11px] text-transparent">四</span>
              <span className="h-[11px] text-[9px] leading-[11px] text-subtle font-mono">五</span>
              <span className="h-[11px] text-[9px] leading-[11px] text-transparent">六</span>
            </div>

            {/* 52 Columns */}
            <div className="flex gap-[3px]">
              {weeks.map((week) => (
                <div key={week.weekIndex} className="flex flex-col gap-[3px]">
                  {week.days.map((day) => {
                    if (day.isFuture) {
                      return <div key={day.date} className="size-[11px] invisible pointer-events-none" />;
                    }
                    const tokens = day.data?.totalTokens ?? 0;
                    const models = day.data?.models ?? [];
                    const hasModels = models.length > 0;

                    return (
                      <div
                        key={day.date}
                        className={cn(
                          "size-[11px] rounded-[2px] cursor-pointer transition-transform hover:scale-125 hover:z-10",
                          getCellClass(tokens),
                          day.isToday && "ring-1 ring-accent"
                        )}
                        onMouseEnter={(e) => {
                          const rect = e.currentTarget.getBoundingClientRect();
                          showTooltip(
                            rect,
                            <div className="flex flex-col gap-2">
                              {/* Header */}
                              <div className="flex items-center justify-between border-b border-border/70 pb-1.5 font-semibold">
                                <span className="font-mono text-xs text-fg">{day.date}</span>
                                <span className="font-mono text-xs text-accent">
                                  {hasModels
                                    ? `${models.length} 个模型活跃`
                                    : tokens > 0
                                    ? `${(tokens / 1000).toFixed(1)}k tokens`
                                    : t("dashboard.heatmap.noData", "无数据")}
                                </span>
                              </div>

                              {/* Per-model breakdown */}
                              {hasModels ? (
                                <div className="flex flex-col gap-2 max-h-72 overflow-y-auto pr-0.5">
                                  {models.map((m) => (
                                    <div
                                      key={`${m.provider}-${m.modelId}`}
                                      className="flex flex-col gap-1 rounded bg-sidebar/50 p-1.5 border border-border/40 text-[11px]"
                                    >
                                      <div className="flex items-center justify-between font-semibold">
                                        <div className="flex items-center gap-1.5 truncate max-w-[170px]">
                                          <Cpu className="size-3 text-accent shrink-0" />
                                          <span className="truncate text-fg">{m.modelId}</span>
                                          {m.provider && (
                                            <span className="text-[10px] text-subtle font-mono">
                                              ({providerLabel(m.provider)})
                                            </span>
                                          )}
                                        </div>
                                        <span className="font-mono text-accent">
                                          {(m.totalTokens / 1000).toFixed(1)}k
                                        </span>
                                      </div>

                                      <div className="grid grid-cols-2 gap-x-2 gap-y-0.5 text-[10px]">
                                        <div className="flex justify-between text-subtle">
                                          <span>未缓存输入:</span>
                                          <span className="font-mono text-fg">{m.inputTokens.toLocaleString()}</span>
                                        </div>
                                        <div className="flex justify-between text-subtle">
                                          <span>缓存命中(读):</span>
                                          <span className="font-mono text-emerald-500 font-medium">+{m.cacheReadTokens.toLocaleString()}</span>
                                        </div>
                                        <div className="flex justify-between text-subtle">
                                          <span>缓存写入:</span>
                                          <span className="font-mono text-purple-400 font-medium">{m.cacheWriteTokens.toLocaleString()}</span>
                                        </div>
                                        <div className="flex justify-between text-subtle">
                                          <span>模型输出:</span>
                                          <span className="font-mono text-amber-500 font-medium">{m.outputTokens.toLocaleString()}</span>
                                        </div>
                                      </div>

                                      {(m.costUsd > 0 || m.equivCostUsd > 0) && (
                                        <div className="flex justify-between border-t border-border/40 pt-1 text-[10px] text-muted">
                                          <span>费用 (等效/实际):</span>
                                          <span className="font-mono font-medium text-fg">
                                            ${m.equivCostUsd.toFixed(3)} / ${m.costUsd.toFixed(3)}
                                          </span>
                                        </div>
                                      )}
                                    </div>
                                  ))}
                                </div>
                              ) : tokens > 0 && day.data ? (
                                <div className="flex flex-col gap-1 text-[11px]">
                                  <div className="flex justify-between text-subtle">
                                    <span>未缓存输入:</span>
                                    <span className="font-mono text-fg">{day.data.inputTokens.toLocaleString()}</span>
                                  </div>
                                  <div className="flex justify-between text-subtle">
                                    <span>缓存命中 (读):</span>
                                    <span className="font-mono text-emerald-500 font-medium">+{day.data.cacheReadTokens.toLocaleString()}</span>
                                  </div>
                                  <div className="flex justify-between text-subtle">
                                    <span>缓存写入:</span>
                                    <span className="font-mono text-purple-400 font-medium">{day.data.cacheWriteTokens.toLocaleString()}</span>
                                  </div>
                                  <div className="flex justify-between text-subtle">
                                    <span>模型输出:</span>
                                    <span className="font-mono text-amber-500 font-medium">{day.data.outputTokens.toLocaleString()}</span>
                                  </div>
                                </div>
                              ) : (
                                <p className="text-[11px] text-subtle">
                                  {t("dashboard.heatmap.noData", "当日无活跃记录（空心方块）")}
                                </p>
                              )}

                              {/* Day total footer if multiple models */}
                              {hasModels && models.length > 1 && (
                                <div className="flex items-center justify-between border-t border-border/60 pt-1 text-[10px] text-muted">
                                  <span>当日全量合计:</span>
                                  <span className="font-mono font-semibold text-fg">
                                    {(day.data!.totalTokens / 1000).toFixed(1)}k tokens
                                    {(day.data!.costUsd > 0 || day.data!.equivCostUsd > 0) && ` · $${(day.data!.equivCostUsd || day.data!.costUsd).toFixed(2)}`}
                                  </span>
                                </div>
                              )}
                            </div>,
                            280
                          );
                        }}
                        onMouseLeave={hideTooltip}
                      />
                    );
                  })}
                </div>
              ))}
            </div>
          </div>

          {/* Footer Legend */}
          <div className="mt-3 flex items-center justify-end gap-1.5 text-[10px] text-muted select-none">
            <span>{t("dashboard.heatmap.less", "少")}</span>
            <span className="size-[11px] rounded-[2px] border border-border/80 dark:border-border/60 bg-transparent" />
            <span className="size-[11px] rounded-[2px] bg-emerald-200 dark:bg-emerald-950/90 border border-emerald-300 dark:border-emerald-800/50" />
            <span className="size-[11px] rounded-[2px] bg-emerald-400 dark:bg-emerald-800 border border-emerald-500/60 dark:border-emerald-700/60" />
            <span className="size-[11px] rounded-[2px] bg-emerald-500 dark:bg-emerald-600" />
            <span className="size-[11px] rounded-[2px] bg-emerald-600 dark:bg-emerald-400" />
            <span>{t("dashboard.heatmap.more", "多")}</span>
          </div>
        </div>
      </div>
    </Card>
  );
}

/**
 * 6. Provider & Model Breakdown Bar Chart:
 * Displays bars for each measurable provider and their active models.
 * Mouseover tooltip shows uncached input, cache read, cache write, output, actual and equiv cost.
 */
function ProviderModelBarChart({ providers }: { providers: DashboardProviderInfo[] }) {
  const { t } = useTranslation();
  const { showTooltip, hideTooltip } = useDashboardTooltip();

  // Flatten provider models for comparison
  const modelEntries: {
    provider: ProviderId;
    providerName: string;
    model: DashboardModelUsage;
  }[] = [];

  for (const p of providers) {
    if (p.models && p.models.length > 0) {
      for (const m of p.models) {
        modelEntries.push({
          provider: p.provider,
          providerName: providerLabel(p.provider),
          model: m,
        });
      }
    }
  }

  if (modelEntries.length === 0) {
    return null;
  }

  // Sort by total tokens descending
  modelEntries.sort(
    (a, b) =>
      b.model.inputTokens + b.model.cacheReadTokens + b.model.cacheWriteTokens + b.model.outputTokens -
      (a.model.inputTokens + a.model.cacheReadTokens + a.model.cacheWriteTokens + a.model.outputTokens)
  );

  const maxTokens = Math.max(
    1,
    ...modelEntries.map(
      (e) =>
        e.model.inputTokens +
        e.model.cacheReadTokens +
        e.model.cacheWriteTokens +
        e.model.outputTokens
    )
  );

  return (
    <Card className="p-5">
      <div className="mb-4 flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-2">
          <BarChart3 className="size-4 text-accent" />
          <h3 className="text-sm font-semibold">{t("dashboard.tokens.modelChartTitle", "各 Provider 模型明细柱状图")}</h3>
        </div>
        <p className="text-xs text-subtle">
          {t("dashboard.tokens.modelChartSubtitle", "各模型消耗的 Token 构成与等效/实际金额明细 (悬停查看详情)")}
        </p>
      </div>

      <div className="flex flex-col gap-3">
        {modelEntries.map(({ provider, providerName, model }) => {
          const total =
            model.inputTokens +
            model.cacheReadTokens +
            model.cacheWriteTokens +
            model.outputTokens;
          const widthPct = Math.max(3, (total / maxTokens) * 100);

          return (
            <div
              key={`${provider}-${model.modelId}`}
              className="flex flex-col gap-1 text-xs cursor-pointer rounded p-1 hover:bg-hover/30 transition-colors"
              onMouseEnter={(e) => {
                const rect = e.currentTarget.getBoundingClientRect();
                showTooltip(
                  rect,
                  <div>
                    <div className="flex items-center justify-between border-b border-border/70 pb-1.5 font-semibold">
                      <span className="truncate max-w-[170px] text-fg font-medium">{model.modelId}</span>
                      <span className="font-mono text-accent">{(total / 1000).toFixed(1)}k tokens</span>
                    </div>
                    <div className="mt-2 flex flex-col gap-1 text-[11px]">
                      <div className="flex justify-between text-subtle">
                        <span>渠道:</span>
                        <span className="font-medium text-fg">{providerName}</span>
                      </div>
                      <div className="flex justify-between text-subtle">
                        <span>调用次数:</span>
                        <span className="font-mono text-fg">{model.calls} 次</span>
                      </div>
                      <div className="flex justify-between text-subtle">
                        <span>未缓存输入:</span>
                        <span className="font-mono text-fg">{model.inputTokens.toLocaleString()}</span>
                      </div>
                      <div className="flex justify-between text-subtle">
                        <span>缓存命中 (读):</span>
                        <span className="font-mono text-emerald-500 font-medium">+{model.cacheReadTokens.toLocaleString()}</span>
                      </div>
                      <div className="flex justify-between text-subtle">
                        <span>缓存写入:</span>
                        <span className="font-mono text-purple-400 font-medium">{model.cacheWriteTokens.toLocaleString()}</span>
                      </div>
                      <div className="flex justify-between text-subtle">
                        <span>模型输出:</span>
                        <span className="font-mono text-amber-500 font-medium">{model.outputTokens.toLocaleString()}</span>
                      </div>
                      <div className="mt-1 flex justify-between border-t border-border/50 pt-1 text-[11px]">
                        <span className="text-subtle">等效 / 实际费用:</span>
                        <span className="font-mono font-medium text-fg">
                          ${model.equivCostUsd.toFixed(3)} / ${model.costUsd.toFixed(3)}
                        </span>
                      </div>
                    </div>
                  </div>,
                  270
                );
              }}
              onMouseLeave={hideTooltip}
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <ProviderLogo id={provider} name={providerName} className="size-3.5" />
                  <span className="font-medium text-fg">{model.modelId}</span>
                  <span className="text-[11px] text-subtle font-mono">({providerName})</span>
                </div>
                <div className="flex items-center gap-3 font-mono text-[11px]">
                  <span className="text-subtle">{model.calls} 次调用</span>
                  <span className="font-semibold text-fg">{(total / 1000).toFixed(1)}k tokens</span>
                  {(model.equivCostUsd > 0 || model.costUsd > 0) && (
                    <span className="text-accent">${(model.equivCostUsd || model.costUsd).toFixed(2)}</span>
                  )}
                </div>
              </div>

              {/* Stacked bar */}
              <div className="flex h-3 w-full overflow-hidden rounded bg-border/40">
                <div
                  style={{ width: `${widthPct}%` }}
                  className="flex h-full overflow-hidden rounded transition-all hover:brightness-110"
                >
                  <div
                    style={{ width: total > 0 ? `${(model.inputTokens / total) * 100}%` : "0%" }}
                    className="bg-blue-400"
                    title={`Input: ${(model.inputTokens / 1000).toFixed(1)}k`}
                  />
                  <div
                    style={{ width: total > 0 ? `${(model.cacheReadTokens / total) * 100}%` : "0%" }}
                    className="bg-accent"
                    title={`Cache Read: ${(model.cacheReadTokens / 1000).toFixed(1)}k`}
                  />
                  <div
                    style={{ width: total > 0 ? `${(model.cacheWriteTokens / total) * 100}%` : "0%" }}
                    className="bg-purple-500"
                    title={`Cache Write: ${(model.cacheWriteTokens / 1000).toFixed(1)}k`}
                  />
                  <div
                    style={{ width: total > 0 ? `${(model.outputTokens / total) * 100}%` : "0%" }}
                    className="bg-amber-500"
                    title={`Output: ${(model.outputTokens / 1000).toFixed(1)}k`}
                  />
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </Card>
  );
}

function ProviderCard({ provider }: { provider: DashboardProviderInfo }) {
  const { t } = useTranslation();
  const { showTooltip, hideTooltip } = useDashboardTooltip();
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

  // External gateway check
  const isGatewayUnknown = provider.isGateway || provider.billing === "UNKNOWN";

  return (
    <Card className={cn("relative flex flex-col justify-between p-4 transition-all", isMelted && "border-rose-500/50 bg-rose-500/5")}>
      <div>
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-2">
            <ProviderLogo id={provider.provider} name={providerLabel(provider.provider)} className="size-5" />
            <span className="font-semibold text-xs tracking-tight">{providerLabel(provider.provider)}</span>
          </div>

          {/* Billing Badge with Tooltip for Gateway / Unknown */}
          {isGatewayUnknown ? (
            <span
              className="inline-flex cursor-help"
              onMouseEnter={(e: React.MouseEvent<HTMLSpanElement>) => {
                const rect = e.currentTarget.getBoundingClientRect();
                showTooltip(
                  rect,
                  <div className="flex flex-col gap-1 text-[11px]">
                    <div className="font-semibold text-fg">
                      {t("dashboard.providers.gatewayTooltip", "该渠道使用外部网关，无法知晓计费方式")}
                    </div>
                    {provider.customEndpoint && (
                      <div className="mt-1 rounded bg-sidebar p-1.5 font-mono text-[10px] text-subtle break-all">
                        {provider.customEndpoint}
                      </div>
                    )}
                  </div>,
                  260
                );
              }}
              onMouseLeave={hideTooltip}
            >
              <Badge
                variant="muted"
                className="text-[10px] font-mono bg-subtle/20 text-subtle border border-border"
              >
                {t("dashboard.providers.unknownBilling", "未知")}
              </Badge>
            </span>
          ) : (
            <Badge
              variant={provider.billing === "SUBSCRIPTION" ? "default" : "muted"}
              className="text-[10px] font-mono"
            >
              {provider.billing === "SUBSCRIPTION" ? "订阅" : "按量"}
            </Badge>
          )}
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

        {/* Model chips if available */}
        {provider.models && provider.models.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1 border-t border-border/40 pt-2">
            {provider.models.slice(0, 3).map((m) => {
              const totalM = m.inputTokens + m.cacheReadTokens + m.cacheWriteTokens + m.outputTokens;
              return (
                <div
                  key={m.modelId}
                  className="flex items-center gap-1 rounded bg-sidebar px-1.5 py-0.5 text-[10px] font-mono text-subtle cursor-pointer hover:bg-hover hover:text-fg transition-colors"
                  onMouseEnter={(e) => {
                    const rect = e.currentTarget.getBoundingClientRect();
                    showTooltip(
                      rect,
                      <div>
                        <div className="font-semibold text-fg border-b border-border/70 pb-1">{m.modelId}</div>
                        <div className="mt-1.5 flex flex-col gap-1 text-[11px]">
                          <div className="flex justify-between text-subtle">
                            <span>输入:</span>
                            <span className="font-mono text-fg">{m.inputTokens.toLocaleString()}</span>
                          </div>
                          <div className="flex justify-between text-subtle">
                            <span>缓存命中:</span>
                            <span className="font-mono text-emerald-500 font-medium">+{m.cacheReadTokens.toLocaleString()}</span>
                          </div>
                          <div className="flex justify-between text-subtle">
                            <span>输出:</span>
                            <span className="font-mono text-amber-500 font-medium">{m.outputTokens.toLocaleString()}</span>
                          </div>
                          <div className="flex justify-between text-subtle">
                            <span>费用:</span>
                            <span className="font-mono text-fg">${m.costUsd.toFixed(3)}</span>
                          </div>
                        </div>
                      </div>,
                      220
                    );
                  }}
                  onMouseLeave={hideTooltip}
                >
                  <Cpu className="size-2.5 text-accent" />
                  <span className="max-w-[70px] truncate">{m.modelId}</span>
                  <span className="text-fg font-semibold">{(totalM / 1000).toFixed(0)}k</span>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Melt Alert or Reset Timer */}
      <div className="border-t border-border/50 pt-2.5 mt-2">
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
    case "REASONIX":
      return "Reasonix";
    default:
      return id;
  }
}

