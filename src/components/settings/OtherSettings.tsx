import { useTranslation } from "react-i18next";
import { useProviders } from "../../stores/providers";
import { useSettings, type ContextMode, type IsolationMode, type RequestMode } from "../../stores/settings";
import { Logo } from "../Logo";
import { Card, NumberInput, Segmented, Select, SettingRow, Switch } from "../ui";

/** Model choices until the knowledge pack (docs/04) is wired: one entry per ready provider. */
function useModelOptions(autoLabel: string) {
  const providers = useProviders((s) => s.providers);
  return [
    { value: "auto", label: autoLabel },
    ...providers.filter((p) => p.status === "READY").map((p) => ({ value: p.id as string, label: p.name })),
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
    </Card>
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
  return (
    <Card>
      <div className="flex items-center gap-4 px-4 py-5">
        <Logo className="size-10" />
        <div>
          <div className="text-[15px] font-semibold">{t("app.name")}</div>
          <div className="text-xs text-muted">{t("settings.about.desc")}</div>
        </div>
      </div>
      <SettingRow title={t("settings.about.version")}>
        <code className="font-mono text-xs text-muted">{__APP_VERSION__}</code>
      </SettingRow>
      <div className="px-4 py-3.5 text-xs text-muted">{t("settings.about.dataDir")}</div>
    </Card>
  );
}
