import { useTranslation } from "react-i18next";
import { useSettings } from "../../stores/settings";
import { prepareDemo, hideDemo, showDemo, useDemoStatus } from "../../lib/demoMode";
import type { MockScenario } from "../../lib/debugProviderTypes";
import { Button, NumberInput, Select, SettingRow, Switch } from "../ui";

export function DemoSettings() {
  const { t } = useTranslation();
  const enabled = useSettings(s => s.demoEnabled);
  const scenario = useSettings(s => s.demoScenario);
  const delay = useSettings(s => s.demoChunkDelayMs);
  const set = useSettings(s => s.set);
  const { busy, error } = useDemoStatus();
  return <div className="mb-6 rounded-lg border border-border">
    <SettingRow title={t("demo.title")} description={t("demo.description")}>
      <Switch label={t("demo.title")} checked={enabled} disabled={busy} onChange={value => {
        if (!value) hideDemo();
        else void prepareDemo().then(() => set("demoEnabled", true)).catch(() => {});
      }} />
    </SettingRow>
    <SettingRow title={t("demo.scenario")} description={t("demo.scenarioDescription")}>
      <Select label={t("demo.scenario")} value={scenario} onChange={value => { if (!busy) set("demoScenario", value as MockScenario); }} options={(["success", "repair", "error", "slow"] as const).map(value => ({ value, label: t(`debugProvider.scenarios.${value}`) }))} />
    </SettingRow>
    <SettingRow title={t("debugProvider.delay")} description={t("debugProvider.delayDesc")}>
      <NumberInput label={t("debugProvider.delay")} value={delay} min={0} max={5000} step={10} suffix="ms" onChange={value => set("demoChunkDelayMs", value)} />
    </SettingRow>
    <div className="flex gap-2 px-4 py-3">
      <Button disabled={!enabled || busy} onClick={showDemo}>{t("demo.open")}</Button>
      <Button disabled={busy} onClick={() => void prepareDemo(true).then(showDemo).catch(() => {})}>{busy ? t("demo.preparing") : t("demo.reset")}</Button>
    </div>
    {error && <p role="alert" className="px-4 pb-3 text-xs text-warning">{t("demo.prepareError", { error })}</p>}
  </div>;
}
