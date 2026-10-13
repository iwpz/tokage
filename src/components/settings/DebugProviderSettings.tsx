import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../stores/settings";
import { useProviders } from "../../stores/providers";
import { type DebugOutput, type DebugProviderConfig, type MockScenario } from "../../lib/debugProviderTypes";
import { NumberInput, Segmented, Select, SettingRow, Button } from "../ui";
import { inTauri, openPathIn, prepareDebugWorkspace } from "../../lib/ipc";
import { useProjects } from "../../stores/projects";

export function DebugProviderSettings() {
  const { t } = useTranslation();
  const config = useSettings((s) => s.debugProvider);
  const set = useSettings((s) => s.set);
  const real = useProviders((s) => s.realProviders);
  const projectPath = useProjects((s) => s.projects.find((p) => p.id === s.activeProjectId)?.path);
  const [workspaceError, setWorkspaceError] = useState<string | null>(null);
  const update = (patch: Partial<DebugProviderConfig>) => set("debugProvider", { ...config, ...patch });
  const target = config.output === "luna" || config.output === "deepseek" ? config.output : null;
  const targetProvider = target === "luna" ? config.lunaProvider : config.deepseekProvider;
  const targetModel = target === "luna" ? config.lunaModel : config.deepseekModel;
  const provider = real.find((p) => p.id === targetProvider);
  const cliChoices = [...new Map([
    { value: targetProvider, label: provider?.name ?? targetProvider },
    ...real.filter((p) => p.chat).map((p) => ({ value: p.id as string, label: p.name })),
    ...["CODEX_CLI", "CURSOR_CLI", "COPILOT_CLI", "AIDER", "OPENCODE", "REASONIX"].map((id) => ({ value: id, label: real.find((p) => p.id === id)?.name ?? id })),
  ].map((p) => [p.value, p])).values()];
  const inputClass = "w-64 rounded-md border border-border bg-input px-2 py-1.5 text-xs text-fg";

  return (
    <>
      <SettingRow title={t("debugProvider.title")} description={t("debugProvider.description")}>
        <Segmented<DebugOutput> label={t("debugProvider.title")} value={config.output} onChange={(output) => update({ output })}
          options={(["off", "mock", "luna", "deepseek"] as const).map((value) => ({ value, label: t(`debugProvider.outputs.${value}`) }))} />
      </SettingRow>
      {config.output === "mock" && (
        <>
          <SettingRow title={t("debugProvider.scenario")} description={t("debugProvider.scenarioDesc")}>
            <Select label={t("debugProvider.scenario")} value={config.scenario} onChange={(scenario) => update({ scenario: scenario as MockScenario })}
              options={(["success", "repair", "error", "slow"] as const).map((value) => ({ value, label: t(`debugProvider.scenarios.${value}`) }))} />
          </SettingRow>
          <SettingRow title={t("debugProvider.delay")} description={t("debugProvider.delayDesc")}>
            <NumberInput label={t("debugProvider.delay")} value={config.chunkDelayMs} min={0} max={5000} step={10} suffix="ms" onChange={(chunkDelayMs) => update({ chunkDelayMs })} />
          </SettingRow>
          <SettingRow title={t("debugProvider.reply")} description={t("debugProvider.replyDesc")}>
            <textarea aria-label={t("debugProvider.reply")} className={inputClass} rows={3} value={config.mockReply} onChange={(e) => update({ mockReply: e.target.value })} />
          </SettingRow>
          <SettingRow title={t("debugProvider.directory")} description={t("debugProvider.directoryDesc")}>
            <Button disabled={!inTauri || !projectPath} onClick={async () => {
              if (!projectPath) return;
              setWorkspaceError(null);
              try { await openPathIn(await prepareDebugWorkspace(projectPath), "FILE_MANAGER"); }
              catch (error) { setWorkspaceError(String(error)); }
            }}>{t("debugProvider.openDirectory")}</Button>
          </SettingRow>
          {workspaceError && <p role="alert" className="px-4 py-2 text-xs text-warning">{workspaceError}</p>}
        </>
      )}
      {target && (
        <>
          <SettingRow title={t("debugProvider.targetCli")} description={t("debugProvider.targetCliDesc")}>
            <Select label={t("debugProvider.targetCli")} value={targetProvider} options={cliChoices}
              onChange={(value) => update(target === "luna" ? { lunaProvider: value } : { deepseekProvider: value })} />
          </SettingRow>
          <SettingRow title={t("debugProvider.targetModel")} description={t("debugProvider.paidDescription")}>
            <input aria-label={t("debugProvider.targetModel")} className={inputClass} value={targetModel}
              onChange={(e) => update(target === "luna" ? { lunaModel: e.target.value } : { deepseekModel: e.target.value })} />
          </SettingRow>
          {(!provider || !["READY", "UNSUPPORTED_VERSION"].includes(provider.status)) && (
            <p role="status" className="px-4 py-2 text-xs text-warning">{t("debugProvider.targetUnavailable", { provider: targetProvider })}</p>
          )}
        </>
      )}
    </>
  );
}
