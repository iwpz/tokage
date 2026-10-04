import { Fragment } from "react";
import { ExternalLink } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useProviders } from "../../stores/providers";
import { useSettings, type ContextMode, type IsolationMode, type RequestMode } from "../../stores/settings";
import { useBuildInfo } from "../../lib/build";
import { openExternalUrl } from "../../lib/ipc";
import { runnable } from "../../lib/models";
import { Logo } from "../Logo";
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

      <Card>
        <div className="px-4 pt-3.5 text-[13px] font-medium">{t("settings.about.verify.title")}</div>
        <p className="px-4 pt-1 pb-2 text-xs leading-relaxed text-muted">{t("settings.about.verify.desc")}</p>
        <dl className="grid grid-cols-[9rem_1fr] gap-y-1.5 px-4 pb-3.5 text-xs">
          {(["sha256", "teamId", "winSigner"] as const).map((k) => (
            <Fragment key={k}>
              <dt className="text-subtle">{t(`settings.about.verify.${k}`)}</dt>
              <dd className="text-muted">{t("settings.about.verify.pending")}</dd>
            </Fragment>
          ))}
        </dl>
      </Card>
    </div>
  );
}
