import { useState, useMemo, useCallback, useEffect } from "react";
import { Ban, ExternalLink, Sliders, Trash2, Zap } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useProviders } from "../../stores/providers";
import { useSettings, type ContextMode, type IsolationMode, type RequestMode } from "../../stores/settings";
import { useBuildInfo } from "../../lib/build";
import {
  cleanupRetentionRecords,
  listOrphanWorktrees,
  openExternalUrl,
  removeOrphanWorktree,
  type OrphanWorktreeEntry,
} from "../../lib/ipc";
import {
  catalogFamilyKey,
  channelsForFamily,
  familyOf,
  isFamilyAllowedOnChannel,
  matrixFamilyRows,
  probedCatalogModels,
  runnable,
  ULTRA_FAMILY_KEYS,
} from "../../lib/models";
import { cn } from "../../lib/cn";
import { Logo } from "../Logo";
import { DebugProviderSettings } from "./DebugProviderSettings";
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
          options={(["auto", "plan", "direct", "fixed"] as const).map((m) => ({
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
      <DebugProviderSettings />
      <ModelChannelMatrixCard />
    </Card>
  );
}


function ModelChannelMatrixCard() {
  const { t } = useTranslation();
  const providers = useProviders((s) => s.providers);
  const access = useSettings((s) => s.modelChannelAccess);
  const setFamilyChannelAllowed = useSettings((s) => s.setFamilyChannelAllowed);
  const setFamilyPrefer = useSettings((s) => s.setFamilyPrefer);
  const banUltraExpensiveFamilies = useSettings((s) => s.banUltraExpensiveFamilies);
  const addModelFamilyFromCatalog = useSettings((s) => s.addModelFamilyFromCatalog);
  const removeCustomModelFamily = useSettings((s) => s.removeCustomModelFamily);

  const availableProviders = useMemo(() => providers.filter(runnable), [providers]);
  const rows = useMemo(() => matrixFamilyRows(access), [access]);
  const catalog = useMemo(() => probedCatalogModels(availableProviders), [availableProviders]);
  const presentKeys = useMemo(() => new Set(rows.map((r) => r.key)), [rows]);

  const addableCatalog = useMemo(
    () =>
      catalog.filter((m) => {
        const resolved =
          familyOf(m.key, access) ?? familyOf(m.name, access) ?? catalogFamilyKey(m.key, m.name);
        return !presentKeys.has(resolved);
      }),
    [catalog, access, presentKeys],
  );

  const hasUltraBanned = ULTRA_FAMILY_KEYS.some((key) => access[key]?.denyAll);

  return (
    <div className="border-t border-border/60 px-4 py-3.5">
      <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-1.5">
            <Sliders className="size-3.5 text-accent" />
            <span className="text-[13px] font-medium text-fg">
              {t("settings.planning.channelMatrix.title")}
            </span>
          </div>
          <p className="mt-0.5 text-xs text-muted leading-relaxed">
            {t("settings.planning.channelMatrix.desc")}
          </p>
        </div>
        <Button
          size="sm"
          variant="secondary"
          onClick={() => banUltraExpensiveFamilies()}
          title={t("settings.planning.channelMatrix.banUltraTitle")}
          className="shrink-0 gap-1.5 text-xs h-7 border-amber-500/30 hover:border-amber-500/60 text-amber-500"
        >
          <Zap className="size-3 text-amber-500" />
          <span>
            {hasUltraBanned
              ? t("settings.planning.channelMatrix.banUltraDone")
              : t("settings.planning.channelMatrix.banUltra")}
          </span>
        </Button>
      </div>

      <div className="mt-3 flex flex-col sm:flex-row sm:items-center gap-2">
        <span className="text-muted text-[11px] shrink-0">
          {t("settings.planning.channelMatrix.addFromCatalog")}
        </span>
        <select
          value=""
          onChange={(e) => {
            const raw = e.target.value;
            if (!raw) return;
            const item = catalog.find((m) => m.key === raw);
            if (!item) return;
            addModelFamilyFromCatalog(item.key, item.name);
            e.target.value = "";
          }}
          disabled={addableCatalog.length === 0}
          className="h-8 min-w-0 flex-1 rounded border border-border bg-elevated px-2 text-[11px] text-fg focus:border-ring focus:outline-none disabled:opacity-50"
        >
          <option value="">
            {catalog.length === 0
              ? t("settings.planning.channelMatrix.catalogEmpty")
              : addableCatalog.length === 0
                ? t("settings.planning.channelMatrix.catalogAllPresent")
                : t("settings.planning.channelMatrix.catalogPick")}
          </option>
          {addableCatalog.map((m) => (
            <option key={m.key} value={m.key}>
              {m.name}
              {m.name !== m.key ? ` (${m.key})` : ""}
            </option>
          ))}
        </select>
      </div>

      <div className="mt-3 space-y-3">
        {rows.map((family) => {
          const entry = access[family.key] || { channels: {} };
          const preferProvider = entry.prefer || "";
          const channels = channelsForFamily(availableProviders, family.key, access);
          const denyAll = !!entry.denyAll;

          return (
            <div
              key={family.key}
              className="flex flex-col gap-2.5 rounded-lg border border-border/80 bg-base/50 p-3 text-xs"
            >
              <div className="flex flex-wrap items-center justify-between gap-1.5 border-b border-border/40 pb-2">
                <div className="flex items-center gap-2 min-w-0">
                  <span className="font-semibold text-fg">{family.name}</span>
                  <span className="rounded bg-hover px-1.5 py-0.5 text-[10px] text-subtle font-mono">
                    {family.key}
                  </span>
                  {family.ultra && (
                    <span className="rounded border border-amber-500/30 bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-500">
                      {t("settings.planning.channelMatrix.ultraBadge")}
                    </span>
                  )}
                  {family.custom && (
                    <span className="rounded border border-border/60 bg-hover px-1.5 py-0.5 text-[10px] text-muted">
                      {t("settings.planning.channelMatrix.customBadge")}
                    </span>
                  )}
                  {denyAll && (
                    <span className="rounded border border-danger/40 bg-danger/15 px-1.5 py-0.5 text-[10px] text-danger">
                      {t("settings.planning.channelMatrix.denyAllBadge")}
                    </span>
                  )}
                  {family.custom && (
                    <button
                      type="button"
                      onClick={() => removeCustomModelFamily(family.key)}
                      className="inline-flex items-center gap-0.5 rounded border border-border/60 px-1.5 py-0.5 text-[10px] text-subtle hover:border-danger/40 hover:text-danger"
                      title={t("settings.planning.channelMatrix.removeCustomTitle")}
                    >
                      <Trash2 className="size-3" />
                      {t("settings.planning.channelMatrix.removeCustom")}
                    </button>
                  )}
                </div>

                <div className="flex items-center gap-1.5 shrink-0">
                  <span className="text-muted text-[11px]">
                    {t("settings.planning.channelMatrix.preferChannel")}
                  </span>
                  <select
                    value={preferProvider}
                    onChange={(e) => setFamilyPrefer(family.key, e.target.value || undefined)}
                    className="h-7 rounded border border-border bg-elevated px-2 text-[11px] text-fg focus:border-ring focus:outline-none"
                  >
                    <option value="">{t("settings.planning.channelMatrix.preferAuto")}</option>
                    {channels.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              <div className="flex flex-col sm:flex-row sm:items-center gap-1.5 pt-0.5">
                <span className="text-muted text-[11px] shrink-0">
                  {t("settings.planning.channelMatrix.allowChannels")}
                </span>
                <div className="flex flex-wrap items-center gap-1.5">
                  {channels.length === 0 ? (
                    <span className="text-[11px] text-subtle">
                      {t("settings.planning.channelMatrix.noChannels")}
                    </span>
                  ) : (
                    channels.map((p) => {
                      const allowed = isFamilyAllowedOnChannel(access, family.key, p.id);
                      const otherIds = channels.map((c) => c.id);
                      return (
                        <button
                          key={p.id}
                          type="button"
                          onClick={() =>
                            setFamilyChannelAllowed(family.key, p.id, !allowed, otherIds)
                          }
                          className={cn(
                            "inline-flex items-center gap-1 rounded border px-2 py-1 text-[11px] transition-colors cursor-pointer",
                            allowed
                              ? "border-border/60 bg-elevated text-fg hover:border-border"
                              : "border-danger/40 bg-danger/15 text-danger font-medium",
                          )}
                          title={
                            allowed
                              ? t("settings.planning.channelMatrix.denyTitle", {
                                  channel: p.name,
                                  family: family.name,
                                })
                              : t("settings.planning.channelMatrix.allowTitle", {
                                  channel: p.name,
                                  family: family.name,
                                })
                          }
                        >
                          {!allowed && <Ban className="size-3 shrink-0" />}
                          <span>{p.name}</span>
                          {!allowed && (
                            <span className="ml-0.5 text-[10px] opacity-75 font-mono">
                              {t("settings.planning.channelMatrix.banned")}
                            </span>
                          )}
                        </button>
                      );
                    })
                  )}
                </div>
              </div>
            </div>
          );
        })}
      </div>
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
      <SettingRow title={t("settings.context.minSaving")} description={t("settings.context.minSavingDesc")}>
        <NumberInput
          label={t("settings.context.minSaving")}
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
  const [orphans, setOrphans] = useState<OrphanWorktreeEntry[]>([]);
  const [orphanBusy, setOrphanBusy] = useState(false);
  const [orphanError, setOrphanError] = useState<string | null>(null);

  const refreshOrphans = useCallback(() => {
    void listOrphanWorktrees()
      .then(setOrphans)
      .catch((err) => setOrphanError(String(err)));
  }, []);

  useEffect(() => {
    refreshOrphans();
  }, [refreshOrphans]);

  const handleRemoveOrphan = async (entry: OrphanWorktreeEntry) => {
    const label = entry.branch || entry.path;
    const ok = window.confirm(
      t("settings.execution.orphanConfirm", { path: label }),
    );
    if (!ok) return;
    setOrphanBusy(true);
    setOrphanError(null);
    try {
      await removeOrphanWorktree(entry.path, entry.branch);
      refreshOrphans();
    } catch (err) {
      setOrphanError(String(err));
    } finally {
      setOrphanBusy(false);
    }
  };

  return (
    <div className="space-y-4">
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
        <SettingRow title={t("settings.execution.agentIdle")} description={t("settings.execution.agentIdleDesc")}>
          <NumberInput
            label={t("settings.execution.agentIdle")}
            value={s.agentIdleTimeoutMin}
            min={1}
            max={480}
            step={5}
            suffix="min"
            onChange={(v) => s.set("agentIdleTimeoutMin", v)}
          />
        </SettingRow>
        <SettingRow title={t("settings.execution.agentHard")} description={t("settings.execution.agentHardDesc")}>
          <NumberInput
            label={t("settings.execution.agentHard")}
            value={s.agentHardTimeoutMin}
            min={5}
            max={1440}
            step={10}
            suffix="min"
            onChange={(v) => s.set("agentHardTimeoutMin", v)}
          />
        </SettingRow>
        <SettingRow title={t("settings.execution.gateEnvAllow")} description={t("settings.execution.gateEnvAllowDesc")}>
          <div className="flex w-full flex-col gap-1.5">
            <textarea
              className="min-h-[4.5rem] w-full rounded-md border border-border bg-base px-2.5 py-1.5 font-mono text-xs text-fg focus:border-accent focus:outline-hidden"
              value={s.gateEnvAllow.join("\n")}
              placeholder={"ANTHROPIC_API_KEY\nMY_TEST_*"}
              onChange={(e) =>
                s.set(
                  "gateEnvAllow",
                  e.target.value
                    .split("\n")
                    .map((line) => line.trim())
                    .filter(Boolean),
                )
              }
            />
            <p className="text-[11px] text-muted">{t("settings.execution.gateEnvAllowWarn")}</p>
          </div>
        </SettingRow>
      </Card>

      <Card>
        <div className="flex items-center justify-between px-4 pt-4 pb-2">
          <div>
            <div className="text-sm font-medium text-fg">{t("settings.execution.orphansTitle")}</div>
            <p className="mt-0.5 text-[11px] text-muted">{t("settings.execution.orphansDesc")}</p>
          </div>
          <Button variant="secondary" size="sm" className="text-xs" onClick={refreshOrphans} disabled={orphanBusy}>
            {t("settings.execution.orphansRefresh")}
          </Button>
        </div>
        {orphanError && (
          <p className="px-4 pb-2 text-[11px] text-danger">{orphanError}</p>
        )}
        {orphans.length === 0 ? (
          <p className="px-4 pb-4 text-[11px] text-muted">{t("settings.execution.orphansEmpty")}</p>
        ) : (
          <ul className="divide-y divide-border border-t border-border">
            {orphans.map((o) => (
              <li key={o.path} className="flex items-center justify-between gap-3 px-4 py-2.5">
                <div className="min-w-0 flex-1">
                  <div className="truncate font-mono text-[11px] text-fg">{o.branch || "—"}</div>
                  <div className="truncate font-mono text-[10px] text-muted">{o.path}</div>
                  {o.dirty && (
                    <span className="text-[10px] text-warning">{t("settings.execution.orphanDirty")}</span>
                  )}
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="shrink-0 gap-1 text-xs text-danger"
                  disabled={orphanBusy}
                  onClick={() => void handleRemoveOrphan(o)}
                >
                  <Trash2 className="size-3.5" />
                  {t("settings.execution.orphanRemove")}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}

export function AboutSettings() {
  const { t } = useTranslation();
  const info = useBuildInfo();
  const code = "font-mono text-xs text-muted";
  const [retentionBusy, setRetentionBusy] = useState(false);
  const [retentionMsg, setRetentionMsg] = useState<string | null>(null);
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
        <SettingRow
          title={t("settings.about.retention.title")}
          description={t("settings.about.retention.desc")}
        >
          <Button
            size="sm"
            disabled={retentionBusy}
            onClick={() => {
              void (async () => {
                setRetentionBusy(true);
                setRetentionMsg(null);
                try {
                  const [logs, events] = await cleanupRetentionRecords();
                  setRetentionMsg(
                    t("settings.about.retention.done", { logs, events }),
                  );
                } catch (e) {
                  setRetentionMsg(t("settings.about.retention.failed", { error: String(e) }));
                } finally {
                  setRetentionBusy(false);
                }
              })();
            }}
          >
            <Trash2 aria-hidden className="size-3.5" />
            {retentionBusy
              ? t("settings.about.retention.running")
              : t("settings.about.retention.run")}
          </Button>
        </SettingRow>
        {retentionMsg && (
          <p className="px-4 pb-3 text-xs text-muted" role="status">
            {retentionMsg}
          </p>
        )}
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
