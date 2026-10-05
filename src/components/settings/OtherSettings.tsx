import { useState, useMemo, useRef } from "react";
import { Ban, ExternalLink, X, Zap } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useProviders } from "../../stores/providers";
import { useSettings, type ContextMode, type IsolationMode, type RequestMode } from "../../stores/settings";
import { useBuildInfo } from "../../lib/build";
import { openExternalUrl, type ProviderId } from "../../lib/ipc";
import { runnable } from "../../lib/models";
import { cn } from "../../lib/cn";
import { Logo } from "../Logo";
import { ProviderLogo } from "../ProviderLogo";
import { SourceNotice } from "../SourceNotice";
import { Button, Card, NumberInput, Segmented, Select, SettingRow, Switch } from "../ui";

/** Model choices until the knowledge pack (docs/04) is wired: one entry per ready provider. */
function useModelOptions(autoLabel: string) {
  const providers = useProviders((s) => s.providers);
  return [
    { value: "auto", label: autoLabel },
    ...providers.filter(runnable).map((p) => ({ value: p.id as string, label: p.name })),
  ];
}

export function PlanningSettings() {
  const { t } = useTranslation();
  const s = useSettings();
  const models = useModelOptions(t("settings.planning.plannerAuto"));

  return (
    <Card>
      <SettingRow title={t("settings.planning.mode")} description={t("settings.planning.modeDesc")}>
        <Segmented<RequestMode>
          label={t("settings.planning.mode")}
          value={s.requestMode}
          onChange={(v) => s.set("requestMode", v)}
          disabled={s.rememberMode}
          options={(["auto", "plan", "direct"] as const).map((m) => ({
            value: m,
            label: t(`workspace.composer.modes.${m}`),
          }))}
        />
      </SettingRow>
      <SettingRow title={t("settings.planning.rememberMode")} description={t("settings.planning.rememberModeDesc")}>
        <Switch label={t("settings.planning.rememberMode")} checked={s.rememberMode} onChange={(v) => s.set("rememberMode", v)} />
      </SettingRow>
      <SettingRow title={t("settings.planning.planner")} description={t("settings.planning.plannerDesc")}>
        <Select label={t("settings.planning.planner")} value={s.plannerModel} options={models} onChange={(v) => s.set("plannerModel", v)} />
      </SettingRow>
      <SettingRow title={t("settings.planning.threshold")} description={t("settings.planning.thresholdDesc")}>
        <NumberInput
          label={t("settings.planning.threshold")}
          value={Math.round(s.confidenceThreshold * 100)}
          min={50}
          max={99}
          suffix="%"
          onChange={(v) => s.set("confidenceThreshold", v / 100)}
        />
      </SettingRow>
      <SettingRow title={t("settings.planning.reference")} description={t("settings.planning.referenceDesc")}>
        <Select label={t("settings.planning.reference")} value={s.referenceModel} options={models} onChange={(v) => s.set("referenceModel", v)} />
      </SettingRow>
      <SettingRow title={t("settings.planning.requireReview")} description={t("settings.planning.requireReviewDesc")}>
        <Switch label={t("settings.planning.requireReview")} checked={s.requireReview} onChange={(v) => s.set("requireReview", v)} />
      </SettingRow>
      <SettingRow title={t("settings.planning.autoApproveBelowUsd")} description={t("settings.planning.autoApproveBelowUsdDesc")}>
        <NumberInput
          label={t("settings.planning.autoApproveBelowUsd")}
          value={s.planningAutoApproveBelowUsd}
          min={0}
          step={0.1}
          suffix="$"
          onChange={(v) => s.set("planningAutoApproveBelowUsd", v)}
        />
      </SettingRow>
      <SettingRow title={t("settings.planning.devShadowModel")} description={t("settings.planning.devShadowModelDesc")}>
        <Segmented<"none" | "luna" | "deepseek">
          label={t("settings.planning.devShadowModel")}
          value={s.devShadowModel}
          onChange={(v) => s.set("devShadowModel", v)}
          options={[
            { value: "none", label: t("settings.planning.devShadowNone") },
            { value: "luna", label: "Luna (gpt-6-luna)" },
            { value: "deepseek", label: "DeepSeek (V4 Pro)" },
          ]}
        />
      </SettingRow>
      <BlockedModelsRow />
    </Card>
  );
}

/** Known ultra-expensive model keywords in standard catalogs (Tier 5 / flagship high price) */
const ULTRA_EXPENSIVE_KEYWORDS = [
  "fable",
  "gpt-6.1-sol",
  "gpt-6-sol",
  "claude-opus-5",
  "claude-opus-4",
];

function BlockedModelsRow() {
  const { t } = useTranslation();
  const blocked = useSettings((s) => s.blockedModels);
  const toggleBlockedModel = useSettings((s) => s.toggleBlockedModel);
  const removeBlockedModel = useSettings((s) => s.removeBlockedModel);
  const providers = useProviders((s) => s.providers);

  const [searchVal, setSearchVal] = useState("");
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const inputContainerRef = useRef<HTMLDivElement>(null);
  const [openUpward, setOpenUpward] = useState(false);

  // Measure space below when opening dropdown to avoid being cut off by the window
  const handleOpenDropdown = () => {
    if (inputContainerRef.current) {
      const rect = inputContainerRef.current.getBoundingClientRect();
      const spaceBelow = window.innerHeight - rect.bottom;
      // If remaining height below is less than 240px and space above is bigger, open upward
      setOpenUpward(spaceBelow < 240 && rect.top > 240);
    }
    setDropdownOpen(true);
  };

  // Aggregate and deduplicate models into:
  // 1. Universal Base Models (cross-provider, e.g. "Claude Fable 5.1")
  // 2. Concrete provider models using modelGroups (grouping cursor variants into one clean entry)
  const allAvailableModels = useMemo(() => {
    interface ModelCandidateItem {
      id: string;
      formatLabel: string;
      isUniversal?: boolean;
      providerId?: ProviderId;
      providerName?: string;
      modelName: string;
    }
    const list: ModelCandidateItem[] = [];
    const seenUniversal = new Set<string>();

    // 1. Gather provider grouped models (utilizing modelGroups to collapse Cursor variants)
    for (const p of providers) {
      if (p.modelGroups && p.modelGroups.length > 0) {
        for (const g of p.modelGroups) {
          const mName = g.name || g.key;
          // Add to universal candidates set
          if (!seenUniversal.has(mName.toLowerCase())) {
            seenUniversal.add(mName.toLowerCase());
            list.push({
              id: `global:${mName}`,
              formatLabel: mName,
              isUniversal: true,
              modelName: mName,
            });
          }
          list.push({
            id: `${p.id}:${g.key}`,
            formatLabel: `${p.name} · ${mName}`,
            providerId: p.id,
            providerName: p.name,
            modelName: mName,
          });
        }
      } else {
        // Fallback for providers without modelGroups
        for (const m of p.models) {
          if (!seenUniversal.has(m.name.toLowerCase())) {
            seenUniversal.add(m.name.toLowerCase());
            list.push({
              id: `global:${m.name}`,
              formatLabel: m.name,
              isUniversal: true,
              modelName: m.name,
            });
          }
          list.push({
            id: `${p.id}:${m.id}`,
            formatLabel: `${p.name} · ${m.name}`,
            providerId: p.id,
            providerName: p.name,
            modelName: m.name,
          });
        }
      }
    }

    return list;
  }, [providers]);

  // Filter based on fuzzy query and exclude already blocked ones
  const filteredCandidates = useMemo(() => {
    const q = searchVal.trim().toLowerCase();
    return allAvailableModels.filter((item) => {
      // Check if already blocked in list
      const alreadyBlocked = blocked.some((b) => {
        const bNorm = b.toLowerCase();
        return (
          bNorm === item.formatLabel.toLowerCase() ||
          bNorm === item.id.toLowerCase() ||
          bNorm === item.modelName.toLowerCase()
        );
      });
      if (alreadyBlocked) return false;
      if (!q) return true;
      return (
        item.formatLabel.toLowerCase().includes(q) ||
        (item.providerName && item.providerName.toLowerCase().includes(q)) ||
        item.modelName.toLowerCase().includes(q)
      );
    });
  }, [allAvailableModels, searchVal, blocked]);

  const handleSelectModel = (formatLabel: string) => {
    if (!blocked.some((b) => b.toLowerCase() === formatLabel.toLowerCase())) {
      toggleBlockedModel(formatLabel);
    }
    setSearchVal("");
    setDropdownOpen(false);
  };

  // One-click ban ultra-expensive models
  const handleBanUltraExpensive = () => {
    const targets = ["Claude Fable 5.1", "GPT-6.1 Sol", "GPT-6 Sol", "Claude Opus 5"];
    targets.forEach((name) => {
      if (!blocked.some((b) => b.toLowerCase() === name.toLowerCase())) {
        toggleBlockedModel(name);
      }
    });
  };

  const hasBannedAnyUltra = blocked.some((b) =>
    ULTRA_EXPENSIVE_KEYWORDS.some((kw) => b.toLowerCase().includes(kw)),
  );

  return (
    <div className="px-4 py-3.5">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <div>
          <div className="text-[13px] font-medium text-fg">{t("settings.planning.blockedModels")}</div>
          <div className="mt-0.5 text-xs leading-relaxed text-muted">
            {t("settings.planning.blockedModelsDesc")}
          </div>
        </div>

        {/* Quick action: One-click ban ultra-expensive models */}
        <Button
          size="sm"
          variant="secondary"
          onClick={handleBanUltraExpensive}
          title="一键屏蔽 Fable 5.1 / GPT-6.1 Sol / Opus 5 等超昂贵旗舰模型"
          className="shrink-0 gap-1.5 text-xs h-7 border-amber-500/30 hover:border-amber-500/60 text-amber-500"
        >
          <Zap className="size-3 text-amber-500" />
          <span>{hasBannedAnyUltra ? "补充屏蔽超昂贵模型" : "一键屏蔽超昂贵模型"}</span>
        </Button>
      </div>

      <div ref={inputContainerRef} className="relative mt-3">
        <div className="flex items-center gap-2">
          <input
            type="text"
            value={searchVal}
            onChange={(e) => {
              setSearchVal(e.target.value);
              handleOpenDropdown();
            }}
            onFocus={handleOpenDropdown}
            placeholder={t("settings.planning.blockedModelsPlaceholder")}
            className="h-8 flex-1 rounded-md border border-border bg-base px-2.5 text-xs text-fg placeholder:text-subtle focus:border-ring focus:outline-none"
          />
          {searchVal && (
            <button
              type="button"
              onClick={() => {
                setSearchVal("");
                setDropdownOpen(false);
              }}
              className="text-xs text-subtle hover:text-fg"
            >
              清除
            </button>
          )}
        </div>

        {/* Search dropdown menu (Supports opening upward if near bottom of window) */}
        {dropdownOpen && (
          <>
            <div
              className="fixed inset-0 z-20"
              onClick={() => setDropdownOpen(false)}
            />
            <div
              className={cn(
                "absolute left-0 z-30 max-h-56 w-full overflow-y-auto rounded-lg border border-border bg-elevated p-1 shadow-xl text-xs",
                openUpward ? "bottom-full mb-1" : "top-full mt-1",
              )}
            >
              {filteredCandidates.length === 0 ? (
                <div className="px-3 py-2 text-xs text-subtle text-center">
                  {searchVal ? "未找到匹配的模型" : "所有可用模型均已添加到屏蔽列表"}
                </div>
              ) : (
                <ul className="space-y-0.5">
                  {filteredCandidates.map((c) => (
                    <li key={c.id}>
                      <button
                        type="button"
                        onClick={() => handleSelectModel(c.formatLabel)}
                        className="flex w-full items-center justify-between rounded-md px-2.5 py-1.5 text-left text-xs text-fg hover:bg-hover focus-visible:bg-hover focus-visible:outline-none"
                      >
                        <div className="flex items-center gap-1.5 min-w-0">
                          {c.isUniversal ? (
                            <span className="shrink-0 rounded bg-accent/15 px-1 py-0.2 text-[10px] font-semibold text-accent">
                              全渠道通用
                            </span>
                          ) : c.providerId ? (
                            <ProviderLogo
                              id={c.providerId}
                              name={c.providerName || ""}
                              className="size-3.5 rounded-xs [&>*]:size-2.5 shrink-0"
                            />
                          ) : null}
                          <span className={cn("truncate", c.isUniversal && "font-semibold text-fg")}>
                            {c.formatLabel}
                          </span>
                        </div>
                        <span className="font-mono text-[10px] text-subtle shrink-0 ml-2">
                          {c.isUniversal ? "屏蔽所有渠道" : c.providerName}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </>
        )}
      </div>

      {blocked.length > 0 && (
        <ul className="mt-2.5 flex flex-wrap gap-1.5">
          {blocked.map((model) => (
            <li
              key={model}
              className="inline-flex items-center gap-1.5 rounded border border-danger/30 bg-danger/10 px-2 py-0.5 text-xs text-danger"
            >
              <Ban className="size-3" />
              <span className="font-medium">{model}</span>
              <button
                type="button"
                aria-label={`Remove ${model}`}
                onClick={() => removeBlockedModel(model)}
                className="ml-0.5 inline-flex size-3.5 items-center justify-center rounded hover:bg-danger/20"
              >
                <X className="size-2.5" />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}


export function ContextSettings() {
  const { t } = useTranslation();
  const s = useSettings();
  return (
    <Card>
      <SettingRow title={t("settings.context.mode")} description={t("settings.context.modeDesc")}>
        <Segmented<ContextMode>
          label={t("settings.context.mode")}
          value={s.contextMode}
          onChange={(v) => s.set("contextMode", v)}
          options={(["auto", "suggest", "off"] as const).map((m) => ({ value: m, label: t(`settings.context.modes.${m}`) }))}
        />
      </SettingRow>
      <SettingRow title={t("settings.context.cacheAware")} description={t("settings.context.cacheAwareDesc")}>
        <Switch label={t("settings.context.cacheAware")} checked={s.cacheAware} onChange={(v) => s.set("cacheAware", v)} />
      </SettingRow>
      <SettingRow title="最低节省门槛" description="离开 CONTINUE 采取压缩或新窗口所需达到的预估节省比例（默认 15%）">
        <NumberInput
          label="最低节省门槛"
          value={Math.round(s.contextMinSaving * 100)}
          min={5}
          max={50}
          suffix="%"
          onChange={(v) => s.set("contextMinSaving", v / 100)}
        />
      </SettingRow>
    </Card>
  );
}

export function BudgetSettings() {
  const { t } = useTranslation();
  const s = useSettings();
  return (
    <Card>
      <SettingRow title={t("settings.budget.planBudget")} description={t("settings.budget.planBudgetDesc")}>
        <NumberInput label={t("settings.budget.planBudget")} value={s.planBudgetUsd} min={0} step={0.5} suffix="$" onChange={(v) => s.set("planBudgetUsd", v)} />
      </SettingRow>
      <SettingRow title={t("settings.budget.overrun")} description={t("settings.budget.overrunDesc")}>
        <Switch label={t("settings.budget.overrun")} checked={s.pauseOnOverrun} onChange={(v) => s.set("pauseOnOverrun", v)} />
      </SettingRow>
      <SettingRow title={t("settings.budget.melt")} description={t("settings.budget.meltDesc")}>
        <NumberInput
          label={t("settings.budget.melt")}
          value={Math.round(s.meltThreshold * 100)}
          min={50}
          max={100}
          suffix="%"
          onChange={(v) => s.set("meltThreshold", v / 100)}
        />
      </SettingRow>
      <SettingRow title={t("settings.budget.activeQuery")} description={t("settings.budget.activeQueryDesc")}>
        <Switch label={t("settings.budget.activeQuery")} checked={s.activeQuotaQuery} onChange={(v) => s.set("activeQuotaQuery", v)} />
      </SettingRow>
    </Card>
  );
}

export function ExecutionSettings() {
  const { t } = useTranslation();
  const s = useSettings();
  return (
    <Card>
      <SettingRow title={t("settings.execution.isolation")} description={t("settings.execution.isolationDesc")}>
        <Select<IsolationMode>
          label={t("settings.execution.isolation")}
          value={s.isolation}
          onChange={(v) => s.set("isolation", v)}
          options={(["worktree", "inplace"] as const).map((m) => ({ value: m, label: t(`settings.execution.isolationModes.${m}`) }))}
        />
      </SettingRow>
      <SettingRow title={t("settings.execution.parallel")} description={t("settings.execution.parallelDesc")}>
        <NumberInput label={t("settings.execution.parallel")} value={s.maxParallel} min={1} max={8} onChange={(v) => s.set("maxParallel", v)} />
      </SettingRow>
      <SettingRow title={t("settings.execution.timeout")} description={t("settings.execution.timeoutDesc")}>
        <NumberInput label={t("settings.execution.timeout")} value={s.timeoutMultiplier} min={0.5} max={5} step={0.5} suffix="×" onChange={(v) => s.set("timeoutMultiplier", v)} />
      </SettingRow>
    </Card>
  );
}

export function AboutSettings() {
  const { t } = useTranslation();
  const info = useBuildInfo();
  const code = "font-mono text-xs text-muted";
  return (
    <div className="space-y-4">
      <Card>
        <div className="flex items-center gap-4 px-4 py-5">
          <Logo className="size-10" />
          <div>
            <div className="text-[15px] font-semibold">{t("app.name")}</div>
            <div className="text-xs text-muted">{t("settings.about.desc")}</div>
          </div>
        </div>
        <SettingRow title={t("settings.about.version")}>
          <code className={code} data-selectable>{info?.version ?? __APP_VERSION__}</code>
        </SettingRow>
        <SettingRow title={t("settings.about.commit")}>
          <code className={code} data-selectable title={info?.commit ?? undefined}>
            {info?.commit?.slice(0, 12) ?? "—"}
          </code>
        </SettingRow>
        <SettingRow
          title={t("settings.about.buildKind")}
          description={info ? t(`settings.about.buildKindDesc.${info.kind}`) : undefined}
        >
          <span className="text-xs text-muted">{info ? t(`settings.about.kinds.${info.kind}`) : "—"}</span>
        </SettingRow>
        <SettingRow title={t("settings.about.source")} description={info?.sourceUrl}>
          <Button size="sm" disabled={!info} onClick={() => info && void openExternalUrl(info.sourceUrl)}>
            <ExternalLink aria-hidden className="size-3.5" />
            {t("settings.about.openSource")}
          </Button>
        </SettingRow>
        <div className="px-4 py-3.5 text-xs text-muted">{t("settings.about.dataDir")}</div>
      </Card>

      <Card>
        <div className="px-4 py-3.5">
          <div className="mb-2 text-[13px] font-medium">{t("welcome.boundary")}</div>
          <SourceNotice variant="full" />
        </div>
      </Card>

      <Card divided={false} className="p-4 space-y-3">
        <div>
          <div className="text-[13px] font-medium">{t("settings.about.verify.title")}</div>
          <p className="mt-1 text-xs leading-relaxed text-muted">{t("settings.about.verify.desc")}</p>
        </div>
        <dl className="grid grid-cols-[8.5rem_1fr] gap-y-2 text-xs">
          <dt className="text-subtle">{t("settings.about.verify.sha256")}</dt>
          <dd className="font-mono text-muted break-all">
            {info?.releaseSha256 ? (
              <a
                href={info.releaseSha256}
                onClick={(e) => {
                  e.preventDefault();
                  void openExternalUrl(info.releaseSha256!);
                }}
                className="text-accent underline hover:opacity-80"
              >
                {info.releaseSha256}
              </a>
            ) : (
              t("settings.about.verify.pending")
            )}
          </dd>

          <dt className="text-subtle">{t("settings.about.verify.teamId")}</dt>
          <dd className="font-mono text-muted">
            {info?.macosTeamId ?? t("settings.about.verify.pending")}
          </dd>

          <dt className="text-subtle">{t("settings.about.verify.winSigner")}</dt>
          <dd className="font-mono text-muted">
            {info?.windowsSigner ?? t("settings.about.verify.pending")}
          </dd>
        </dl>
      </Card>
    </div>
  );
}
