import { formatModelName } from "../../lib/modelDisplay";
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  ChevronDown,
  Coins,
  Download,
  Gauge,
  Layers,
  Loader2,
  Lock,
  RotateCcw,
  Shield,
  ShieldAlert,
  Sparkles,
} from "lucide-react";
import { cn } from "../../lib/cn";
import { listLearnedStats, type LearnedStatRow, type ProviderId } from "../../lib/ipc";
import { parsePack, type KnowledgePack, type PackModel, type PackOffering, type UsageProfileEntry } from "../../lib/knowledge";
import builtinPackText from "../../../src-tauri/resources/knowledge/builtin.json?raw";
import { useBuildInfo } from "../../lib/build";
import { useKnowledge } from "../../stores/knowledge";
import { useProviders } from "../../stores/providers";
import { useSettings } from "../../stores/settings";
import { Button, Card, SettingRow, Switch } from "../ui";

/** The pack shipped with Tokage (docs/04 knowledge-pack §6, `builtin` layer). */
const builtin = parsePack(builtinPackText);

export function KnowledgeSettings() {
  const { t } = useTranslation();
  const s = useSettings();
  const buildInfo = useBuildInfo();
  const knowledgeUpdateEnabled = buildInfo?.knowledgeUpdateEnabled === true;
  const {
    url,
    custom,
    customPacks,
    modelOverrides,
    loading,
    error,
    setUrl,
    load,
    importFromFile,
    removeCustomPack,
    toggleCustomPack,
    moveCustomPack,
    addModelOverride,
    removeModelOverride,
    exportPackJson,
    resetToBuiltin,
    stagedUpdate,
    checkingUpdate,
    applyingUpdate,
    updateMessage,
    checkOfficialUpdate,
    applyOfficialUpdate,
    refreshPendingUpdate,
  } = useKnowledge();
  const [confirming, setConfirming] = useState(false);
  const [selectedLayer, setSelectedLayer] = useState<"effective" | "builtin" | "custom">("effective");
  const [fileError, setFileError] = useState<string | null>(null);
  const [copiedOverrideModel, setCopiedOverrideModel] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    // Local pending.json only — no network.
    void refreshPendingUpdate();
    // F8: official channel gated on knowledge_update_enabled (signing pubkey).
    if (knowledgeUpdateEnabled && s.knowledgeAutoUpdate) {
      void checkOfficialUpdate(builtin.pack.packVersion);
    }
  }, [s.knowledgeAutoUpdate, knowledgeUpdateEnabled]);

  const trimmed = url.trim();
  const valid = /^https:\/\/\S+$/i.test(trimmed) || /^http:\/\/(localhost|127\.0\.0\.1|\[::1\])([:/?#]\S*)?$/i.test(trimmed);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (valid && !loading) setConfirming(true);
  };

  const handleFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setFileError(null);
    const reader = new FileReader();
    reader.onload = () => {
      try {
        const content = String(reader.result ?? "");
        importFromFile(file.name, content);
      } catch (err) {
        setFileError(err instanceof Error ? err.message : String(err));
      }
    };
    reader.onerror = () => {
      setFileError("Failed to read file");
    };
    reader.readAsText(file);
    e.target.value = "";
  };

  const handleExport = (packToExport?: KnowledgePack) => {
    const pack = packToExport ?? {
      ...builtin.pack,
      packVersion: `custom-export-${new Date().toISOString().slice(0, 10)}`,
      models: currentModels,
    };
    const jsonStr = exportPackJson(pack);
    const blob = new Blob([jsonStr], { type: "application/json" });
    const blobUrl = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = blobUrl;
    a.download = `${pack.packVersion || "custom-pack"}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(blobUrl);
  };

  // Compute models & rules based on selected layer and multi-pack stack
  const { currentModels, currentDomains, currentWarnings, currentUsageProfiles, currentRules, isReadOnly } = useMemo(() => {
    if (selectedLayer === "builtin") {
      const models = builtin.pack.models.map((m) => ({ ...m, originLayer: "builtin" }));
      return {
        currentModels: models,
        currentDomains: builtin.pack.domains,
        currentWarnings: builtin.warnings,
        currentUsageProfiles: builtin.pack.usageProfiles,
        currentRules: {
          triage: builtin.pack.triageRules ?? [],
          sandbox: builtin.pack.sandboxProfiles ?? {},
        },
        isReadOnly: true,
      };
    }

    if (selectedLayer === "custom") {
      const enabledCustoms = customPacks.filter((p) => p.enabled);
      const models: PackModel[] = [];
      const warnings: string[] = [];
      const domains = new Set<string>();
      let usageProfiles = {};
      let triageRules: Record<string, unknown>[] = [];
      let sandboxProfiles: Record<string, unknown> = {};

      for (const cp of enabledCustoms) {
        warnings.push(...cp.warnings);
        cp.pack.domains.forEach((d) => domains.add(d));
        usageProfiles = { ...usageProfiles, ...cp.pack.usageProfiles };
        if (cp.pack.triageRules) triageRules = cp.pack.triageRules;
        if (cp.pack.sandboxProfiles) sandboxProfiles = { ...sandboxProfiles, ...cp.pack.sandboxProfiles };
        for (const m of cp.pack.models) {
          models.push({ ...m, originLayer: cp.name });
        }
      }

      return {
        currentModels: models,
        currentDomains: [...domains],
        currentWarnings: warnings,
        currentUsageProfiles: usageProfiles,
        currentRules: {
          triage: triageRules,
          sandbox: sandboxProfiles,
        },
        isReadOnly: false,
      };
    }

    // "effective": Builtin overlaid with all active Custom packs, then overridden by local ModelOverrides
    const modelMap = new Map<string, PackModel>();
    for (const m of builtin.pack.models) {
      modelMap.set(m.modelId, { ...m, originLayer: "builtin" });
    }

    const enabledCustoms = customPacks.filter((p) => p.enabled);
    const combinedWarnings = [...builtin.warnings];
    const combinedDomains = new Set<string>(builtin.pack.domains);
    let combinedProfiles = { ...builtin.pack.usageProfiles };
    let combinedTriage = builtin.pack.triageRules ?? [];
    let combinedSandbox = { ...builtin.pack.sandboxProfiles };

    for (const cp of enabledCustoms) {
      combinedWarnings.push(...cp.warnings);
      cp.pack.domains.forEach((d) => combinedDomains.add(d));
      combinedProfiles = { ...combinedProfiles, ...cp.pack.usageProfiles };
      if (cp.pack.triageRules?.length) combinedTriage = cp.pack.triageRules;
      if (cp.pack.sandboxProfiles) combinedSandbox = { ...combinedSandbox, ...cp.pack.sandboxProfiles };
      for (const m of cp.pack.models) {
        modelMap.set(m.modelId, { ...m, originLayer: cp.name });
      }
    }

    // Apply local model overrides (P6.m3c)
    for (const [mid, ov] of Object.entries(modelOverrides)) {
      const existing = modelMap.get(mid);
      if (existing) {
        modelMap.set(mid, {
          ...existing,
          displayName: ov.displayName ?? existing.displayName,
          skills: ov.skills ? { ...existing.skills, ...ov.skills } : existing.skills,
          price: ov.price ?? existing.price,
          contextWindow: ov.contextWindow ?? existing.contextWindow,
          effectiveContext: ov.effectiveContext ?? existing.effectiveContext,
          speed: ov.speed ?? existing.speed,
          tags: ov.tags ?? existing.tags,
          originLayer: "override",
        });
      }
    }

    return {
      currentModels: [...modelMap.values()],
      currentDomains: [...combinedDomains],
      currentWarnings: combinedWarnings,
      currentUsageProfiles: combinedProfiles,
      currentRules: {
        triage: combinedTriage,
        sandbox: combinedSandbox,
      },
      isReadOnly: true,
    };
  }, [selectedLayer, customPacks, modelOverrides]);

  return (
    <div className="space-y-6">
      <Card>
        <SettingRow title={t("settings.knowledge.version")}>
          <span className="flex items-center gap-2">
            {custom && (
              <span className="rounded bg-hover px-1.5 text-[10px] text-warning">{t("settings.knowledge.thirdParty")}</span>
            )}
            <code className="font-mono text-xs text-muted">{custom ? custom.pack.packVersion : builtin.pack.packVersion}</code>
          </span>
        </SettingRow>
        <SettingRow
          title={t("settings.knowledge.autoUpdate")}
          description={
            knowledgeUpdateEnabled
              ? t("settings.knowledge.autoUpdateDesc")
              : t("settings.knowledge.channelDisabled")
          }
        >
          <Switch
            label={t("settings.knowledge.autoUpdate")}
            checked={knowledgeUpdateEnabled && s.knowledgeAutoUpdate}
            disabled={!knowledgeUpdateEnabled}
            onChange={(v) => s.set("knowledgeAutoUpdate", v)}
          />
        </SettingRow>
        <SettingRow title={t("settings.knowledge.check")}>
          <div className="flex flex-col items-end gap-2">
            <div className="flex items-center gap-3">
              <Button
                onClick={() => void checkOfficialUpdate(builtin.pack.packVersion)}
                disabled={!knowledgeUpdateEnabled || checkingUpdate || applyingUpdate}
              >
                {checkingUpdate ? <Loader2 className="size-3.5 animate-spin" /> : <RotateCcw className="size-3.5" />}
                {t("settings.knowledge.check")}
              </Button>
              {stagedUpdate && knowledgeUpdateEnabled && (
                <Button
                  variant="primary"
                  onClick={() => void applyOfficialUpdate()}
                  disabled={applyingUpdate}
                >
                  {applyingUpdate ? <Loader2 className="size-3.5 animate-spin" /> : <Sparkles className="size-3.5" />}
                  {t("settings.knowledge.apply")} ({stagedUpdate.packVersion})
                </Button>
              )}
            </div>
            {!knowledgeUpdateEnabled && (
              <p className="text-[11px] text-subtle text-right max-w-sm">
                {t("settings.knowledge.channelDisabled")}
              </p>
            )}
          </div>
        </SettingRow>
        {updateMessage && (
          <div className="border-t border-border px-4 py-2.5 text-xs text-subtle flex items-center gap-2">
            <Sparkles className="size-3.5 text-accent" />
            <span>{updateMessage}</span>
          </div>
        )}
      </Card>

      {/* Custom Packs Management Section (P6.m3a, P6.m3b, P6.m3c) */}
      <section aria-labelledby="custom-packs-title" className="space-y-3">
        <div className="flex items-center justify-between px-1">
          <h2 id="custom-packs-title" className="text-xs font-medium text-muted">
            {t("settings.knowledge.custom.multiple")}
          </h2>
          <div className="flex items-center gap-2">
            <input
              type="file"
              ref={fileInputRef}
              onChange={handleFileUpload}
              accept=".json,application/json"
              className="hidden"
            />
            <Button size="sm" onClick={() => fileInputRef.current?.click()}>
              <Download aria-hidden className="size-3.5 rotate-180" />
              {t("settings.knowledge.custom.importFile")}
            </Button>
            <Button size="sm" onClick={() => handleExport()}>
              <Download aria-hidden className="size-3.5" />
              {t("settings.knowledge.custom.exportPack")}
            </Button>
          </div>
        </div>

        <Card>
          <form onSubmit={submit} className="space-y-2 px-4 py-3.5">
            <label htmlFor="pack-url" className="block text-xs leading-relaxed text-muted">
              {t("settings.knowledge.custom.desc")}
            </label>
            <div className="flex gap-2">
              <input
                id="pack-url"
                type="url"
                inputMode="url"
                spellCheck={false}
                autoComplete="off"
                placeholder="https://example.com/tokage-pack.json"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                aria-invalid={trimmed !== "" && !valid}
                aria-describedby={error ? "pack-url-error" : undefined}
                className="h-8 min-w-0 flex-1 rounded-md border border-border bg-elevated px-2 font-mono text-xs text-fg placeholder:text-subtle"
              />
              <Button type="submit" variant="primary" disabled={!valid || loading}>
                {loading ? <Loader2 aria-hidden className="size-3.5 animate-spin" /> : <Download aria-hidden className="size-3.5" />}
                {t("settings.knowledge.custom.load")}
              </Button>
            </div>
            {trimmed !== "" && !valid && <p className="text-xs text-danger">{t("settings.knowledge.custom.invalidUrl")}</p>}
            {error && (
              <p id="pack-url-error" role="alert" className="text-xs break-words text-danger" data-selectable>
                {t("settings.knowledge.custom.failed", { error })}
              </p>
            )}
            {fileError && (
              <p role="alert" className="text-xs break-words text-danger" data-selectable>
                {t("settings.knowledge.custom.invalidFile", { error: fileError })}
              </p>
            )}
          </form>

          {/* Managed Custom Packs List */}
          {customPacks.length > 0 && (
            <div className="border-t border-border divide-y divide-border">
              {customPacks.map((packItem, index) => (
                <div key={packItem.id} className="flex items-center justify-between gap-3 px-4 py-2.5 text-xs">
                  <div className="min-w-0 flex-1 flex items-center gap-2">
                    <span className="rounded bg-hover px-1.5 py-0.5 text-[10px] text-warning border border-warning/30 font-medium">
                      {t("settings.knowledge.custom.unsigned")}
                    </span>
                    <div className="min-w-0">
                      <p className="truncate font-medium text-fg">{packItem.name}</p>
                      <p className="truncate font-mono text-[11px] text-muted">{packItem.url ?? "local JSON"}</p>
                    </div>
                  </div>
                  <div className="flex items-center gap-1">
                    <Button
                      size="sm"
                      disabled={index === 0}
                      onClick={() => moveCustomPack(packItem.id, "up")}
                      title={t("settings.knowledge.custom.moveUp")}
                    >
                      ↑
                    </Button>
                    <Button
                      size="sm"
                      disabled={index === customPacks.length - 1}
                      onClick={() => moveCustomPack(packItem.id, "down")}
                      title={t("settings.knowledge.custom.moveDown")}
                    >
                      ↓
                    </Button>
                    <Button
                      size="sm"
                      variant={packItem.enabled ? "secondary" : "ghost"}
                      onClick={() => toggleCustomPack(packItem.id)}
                    >
                      {packItem.enabled ? t("settings.knowledge.custom.enabled") : t("settings.knowledge.custom.disabled")}
                    </Button>
                    <Button
                      size="sm"
                      onClick={() => removeCustomPack(packItem.id)}
                      title={t("settings.knowledge.custom.remove")}
                    >
                      ✕
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}

          {custom && (
            <div className="flex items-center justify-between bg-hover/40 px-4 py-2 border-t border-border text-xs">
              <span className="text-subtle">
                {t("settings.knowledge.custom.loadedAt", { time: new Date(custom.loadedAt).toLocaleString() })}
              </span>
              <Button size="sm" onClick={resetToBuiltin}>
                <RotateCcw aria-hidden className="size-3.5" />
                {t("settings.knowledge.custom.reset")}
              </Button>
            </div>
          )}
        </Card>
      </section>

      {/* Local Model Overrides Section (P6.m3c) */}
      {Object.keys(modelOverrides).length > 0 && (
        <section aria-labelledby="overrides-title" className="space-y-2">
          <div className="flex items-center justify-between px-1">
            <h2 id="overrides-title" className="text-xs font-medium text-muted">
              {t("settings.knowledge.custom.overridesTitle", { count: Object.keys(modelOverrides).length })}
            </h2>
          </div>
          <Card>
            <div className="divide-y divide-border">
              {Object.values(modelOverrides).map((ov) => (
                <div key={ov.modelId} className="flex items-center justify-between px-4 py-2.5 text-xs">
                  <div>
                    <span className="font-medium text-fg">{formatModelName(ov.displayName || ov.modelId)}</span>
                    <span className="ml-2 font-mono text-[11px] text-subtle">{ov.modelId}</span>
                  </div>
                  <Button size="sm" onClick={() => removeModelOverride(ov.modelId)}>
                    {t("settings.knowledge.custom.remove")}
                  </Button>
                </div>
              ))}
            </div>
          </Card>
        </section>
      )}

      {/* Layer selector bar */}
      <section aria-labelledby="layer-selector-title" className="space-y-2">
        <div className="flex items-center justify-between px-1">
          <div className="flex items-center gap-2">
            <Layers aria-hidden className="size-4 text-muted" />
            <h2 id="layer-selector-title" className="text-xs font-medium text-muted">
              {t("settings.knowledge.layers.label")}
            </h2>
          </div>
          {isReadOnly && (
            <span className="flex items-center gap-1 text-[11px] text-subtle">
              <Lock aria-hidden className="size-3" />
              {t("settings.knowledge.layers.readOnly")}
            </span>
          )}
        </div>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => setSelectedLayer("effective")}
            className={cn(
              "flex flex-1 flex-col items-start rounded-xl border p-3 text-left transition-colors",
              selectedLayer === "effective"
                ? "border-accent bg-accent/5 text-fg shadow-sm"
                : "border-border bg-elevated text-muted hover:border-accent/40",
            )}
          >
            <div className="flex items-center gap-1.5 text-xs font-medium text-fg">
              <Sparkles aria-hidden className="size-3.5 text-accent" />
              {t("settings.knowledge.layers.effective")}
            </div>
            <p className="mt-1 text-[11px] text-subtle">{t("settings.knowledge.layers.effectiveDesc")}</p>
          </button>

          <button
            type="button"
            onClick={() => setSelectedLayer("builtin")}
            className={cn(
              "flex flex-1 flex-col items-start rounded-xl border p-3 text-left transition-colors",
              selectedLayer === "builtin"
                ? "border-accent bg-accent/5 text-fg shadow-sm"
                : "border-border bg-elevated text-muted hover:border-accent/40",
            )}
          >
            <div className="flex items-center gap-1.5 text-xs font-medium text-fg">
              <Shield aria-hidden className="size-3.5 text-muted" />
              {t("settings.knowledge.layers.builtin")}
            </div>
            <p className="mt-1 text-[11px] text-subtle">{t("settings.knowledge.layers.builtinDesc")}</p>
          </button>

          {customPacks.length > 0 && (
            <button
              type="button"
              onClick={() => setSelectedLayer("custom")}
              className={cn(
                "flex flex-1 flex-col items-start rounded-xl border p-3 text-left transition-colors",
                selectedLayer === "custom"
                  ? "border-accent bg-accent/5 text-fg shadow-sm"
                  : "border-border bg-elevated text-muted hover:border-accent/40",
              )}
            >
              <div className="flex items-center gap-1.5 text-xs font-medium text-fg">
                <ShieldAlert aria-hidden className="size-3.5 text-warning" />
                {t("settings.knowledge.layers.custom", { name: customPacks[0]?.name ?? "custom" })}
              </div>
              <p className="mt-1 truncate text-[11px] text-subtle">
                {t("misc.activeCount", { count: customPacks.filter((p) => p.enabled).length })}
              </p>
            </button>
          )}
        </div>
      </section>

      <PackModels
        models={currentModels}
        domains={currentDomains}
        warnings={currentWarnings}
        usageProfiles={currentUsageProfiles}
        onCopyOverride={(m) => {
          addModelOverride(m);
          setCopiedOverrideModel(m.displayName);
          setTimeout(() => setCopiedOverrideModel(null), 2500);
        }}
      />
      {copiedOverrideModel && (
        <div className="fixed bottom-6 right-6 rounded-lg bg-accent px-3 py-2 text-xs font-medium text-accent-fg shadow-lg animate-in fade-in">
          {t("settings.knowledge.custom.overrideAdded", { model: copiedOverrideModel })}
        </div>
      )}

      {/* Rules and Sandbox Profiles Viewer */}
      <section aria-labelledby="rules-section-title" className="space-y-2">
        <h2 id="rules-section-title" className="px-1 text-xs font-medium text-muted">
          {t("settings.knowledge.rules.title")}
        </h2>
        <Card>
          <details className="text-xs">
            <summary className="flex cursor-pointer items-center justify-between px-4 py-3 font-medium text-fg hover:bg-hover/40">
              <span className="flex items-center gap-2">
                <Gauge aria-hidden className="size-4 text-muted" />
                {t("settings.knowledge.rules.triage", { count: currentRules.triage.length })}
              </span>
              <ChevronDown aria-hidden className="size-4 text-subtle" />
            </summary>
            <div className="border-t border-border px-4 py-3 space-y-1.5 max-h-60 overflow-y-auto font-mono text-[11px] text-muted">
              {currentRules.triage.map((rule, idx) => (
                <div key={idx} className="rounded bg-hover/40 p-2">
                  <span className="font-semibold text-fg">{String(rule.id || `rule-${idx}`)}</span>: {t("misc.ruleToward")}{" "}
                  <span className="text-accent">{String(rule.toward)}</span> ({t("misc.ruleWeight", { weight: String(rule.weight) })})
                  {Boolean(rule.pattern) && <div className="text-subtle text-[10px] truncate">{t("misc.rulePattern", { pattern: String(rule.pattern) })}</div>}
                </div>
              ))}
            </div>
          </details>

          <details className="text-xs border-t border-border">
            <summary className="flex cursor-pointer items-center justify-between px-4 py-3 font-medium text-fg hover:bg-hover/40">
              <span className="flex items-center gap-2">
                <Shield aria-hidden className="size-4 text-muted" />
                {t("settings.knowledge.rules.sandbox", { count: Object.keys(currentRules.sandbox).length })}
              </span>
              <ChevronDown aria-hidden className="size-4 text-subtle" />
            </summary>
            <div className="border-t border-border px-4 py-3 space-y-2 max-h-60 overflow-y-auto font-mono text-[11px] text-muted">
              {Object.entries(currentRules.sandbox).map(([cli, prof]) => (
                <div key={cli} className="rounded bg-hover/40 p-2">
                  <div className="font-semibold text-fg">{cli}</div>
                  <pre className="text-[10px] text-subtle overflow-x-auto whitespace-pre-wrap mt-1">
                    {JSON.stringify(prof, null, 2)}
                  </pre>
                </div>
              ))}
            </div>
          </details>
        </Card>
      </section>

      <ThirdPartyDialog
        open={confirming}
        url={trimmed}
        onCancel={() => setConfirming(false)}
        onConfirm={() => {
          setConfirming(false);
          void load(trimmed);
        }}
      />
    </div>
  );
}

/** Shown before every load from a custom URL: Tokage can't vouch for what the pack contains. */
function ThirdPartyDialog({ open, url, onCancel, onConfirm }: { open: boolean; url: string; onCancel: () => void; onConfirm: () => void }) {
  const { t } = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const [ack, setAck] = useState(false);

  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    if (open && !d.open) {
      setAck(false);
      d.showModal();
    }
    if (!open && d.open) d.close();
  }, [open]);

  return (
    <dialog
      ref={dialog}
      aria-labelledby="pack-warn-title"
      aria-describedby="pack-warn-body"
      onCancel={onCancel}
      onClose={() => !dialog.current?.open && open && onCancel()}
      className="m-auto w-[32rem] max-w-[calc(100vw-2rem)] rounded-xl border border-border bg-elevated p-0 text-fg shadow-xl backdrop:bg-black/40"
    >
      <div className="space-y-3 px-5 pt-5 pb-4">
        <h2 id="pack-warn-title" className="flex items-center gap-2 text-[14px] font-medium">
          <AlertTriangle aria-hidden className="size-4 text-warning" />
          {t("settings.knowledge.warn.title")}
        </h2>
        <p className="font-mono text-xs break-all text-subtle" data-selectable>
          {url}
        </p>
        <div id="pack-warn-body" className="space-y-2 text-[13px] leading-relaxed text-muted">
          <p>{t("settings.knowledge.warn.body")}</p>
          <ul className="list-disc space-y-1 pl-5 text-xs">
            <li>{t("settings.knowledge.warn.risk1")}</li>
            <li>{t("settings.knowledge.warn.risk2")}</li>
            <li>{t("settings.knowledge.warn.risk3")}</li>
          </ul>
          <p className="text-xs">{t("settings.knowledge.warn.advice")}</p>
        </div>
        <label className="flex items-start gap-2 text-xs text-fg">
          <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} className="mt-0.5 accent-[var(--accent)]" />
          {t("settings.knowledge.warn.ack")}
        </label>
      </div>
      <div className="flex justify-end gap-2 border-t border-border px-5 py-3">
        <Button onClick={onCancel} autoFocus>
          {t("common.cancel")}
        </Button>
        <Button variant="primary" disabled={!ack} onClick={onConfirm}>
          {t("settings.knowledge.warn.confirm")}
        </Button>
      </div>
    </dialog>
  );
}

function PackModels({
  models,
  domains,
  warnings,
  usageProfiles,
  onCopyOverride,
}: {
  models: PackModel[];
  domains: string[];
  warnings: string[];
  usageProfiles: Record<string, Record<string, UsageProfileEntry>>;
  onCopyOverride?: (model: PackModel) => void;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q
      ? models.filter((m) =>
          [m.displayName, m.modelId, m.vendor ?? "", ...m.tags, ...m.offerings.flatMap((o) => [o.provider, o.cliModelArg])].some((x) =>
            x.toLowerCase().includes(q),
          ),
        )
      : models;
  }, [models, query]);

  return (
    <section aria-labelledby="pack-models-title" className="space-y-2">
      <div className="flex items-center justify-between gap-3 px-1">
        <h2 id="pack-models-title" className="text-xs font-medium text-muted">
          {t("settings.knowledge.models.title", { count: models.length })}
        </h2>
        {models.length > 5 && (
          <input
            type="search"
            aria-label={t("settings.knowledge.models.filter")}
            placeholder={t("settings.knowledge.models.filter")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="h-7 w-48 rounded-md border border-border bg-elevated px-2 text-xs text-fg placeholder:text-subtle"
          />
        )}
      </div>

      {warnings.length > 0 && (
        <details className="rounded-xl border border-border bg-elevated text-xs">
          <summary className="flex cursor-pointer items-center gap-2 px-4 py-2.5 text-warning">
            <AlertTriangle aria-hidden className="size-3.5" />
            {t("settings.knowledge.models.warnings", { count: warnings.length })}
          </summary>
          <ul className="max-h-48 space-y-0.5 overflow-y-auto border-t border-border px-4 py-2 font-mono text-[11px] text-muted" data-selectable>
            {warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </details>
      )}

      {models.length === 0 ? (
        <Card>
          <p className="px-4 py-6 text-center text-xs text-subtle">{t("settings.knowledge.models.empty")}</p>
        </Card>
      ) : (
        <Card>
          {shown.map((m) => (
            <ModelRow
              key={m.modelId}
              model={m}
              domains={domains}
              usageProfiles={usageProfiles}
              onCopyOverride={onCopyOverride}
            />
          ))}
          {shown.length === 0 && <p className="px-4 py-6 text-center text-xs text-subtle">{t("settings.knowledge.models.noMatch")}</p>}
        </Card>
      )}
    </section>
  );
}

const fmtTokens = (n: number) => (n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${Math.round(n / 1000)}K` : String(n));

function ModelRow({
  model: m,
  domains,
  usageProfiles,
  onCopyOverride,
}: {
  model: PackModel;
  domains: string[];
  usageProfiles: Record<string, Record<string, UsageProfileEntry>>;
  onCopyOverride?: (model: PackModel) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [learned, setLearned] = useState<LearnedStatRow[]>([]);
  // Pack domains first, then any extra the model scores.
  const keys = [...domains.filter((d) => d in m.skills), ...Object.keys(m.skills).filter((d) => !domains.includes(d))];
  const top = [...keys].sort((a, b) => m.skills[b] - m.skills[a]).slice(0, 3);
  const facts = [
    m.contextWindow && t("settings.knowledge.models.context", { value: fmtTokens(m.contextWindow) }),
    m.effectiveContext && t("settings.knowledge.models.effective", { value: fmtTokens(m.effectiveContext) }),
    m.price && t("settings.knowledge.models.price", { input: m.price.input, output: m.price.output }),
    m.speed !== null && t("settings.knowledge.models.speed", { value: m.speed }),
  ].filter((x): x is string => !!x);
  const id = `pack-model-${m.modelId}`.replace(/[^\w-]/g, "_");
  const providerName = useProviderName();

  // F14.g: load empirical stats when expanded ("先验 vs 实测")
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void listLearnedStats(m.modelId)
      .then((rows) => {
        if (!cancelled) setLearned(rows);
      })
      .catch(() => {
        if (!cancelled) setLearned([]);
      });
    return () => {
      cancelled = true;
    };
  }, [open, m.modelId]);

  const learnedSamples = learned
    .filter((r) => r.metric === "success_alpha")
    .reduce((sum, r) => sum + r.samples, 0);

  return (
    <div>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-hover/50"
      >
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="text-[13px] font-medium text-fg">{formatModelName(m.displayName)}</span>
            <code className="font-mono text-[11px] text-subtle">{m.modelId}</code>
            {m.originLayer && (
              <span className="rounded border border-border bg-hover px-1.5 text-[10px] text-subtle">
                {m.originLayer}
              </span>
            )}
            {m.offerings.length > 0 && (
              <span className="rounded bg-hover px-1.5 text-[10px] text-muted">
                {t("settings.knowledge.models.providers", { count: m.offerings.length })}
              </span>
            )}
            {m.roles.map((r) => (
              <span key={r} className="rounded bg-accent-soft px-1.5 text-[10px] text-accent">
                {r}
              </span>
            ))}
          </div>
          {!open && (top.length > 0 || m.offerings.length > 0) && (
            <div className="mt-1 truncate text-xs text-muted">
              {[top.map((d) => `${d} ${m.skills[d]}`).join(" · "), m.offerings.map((o) => providerName(o.provider)).join(", ")]
                .filter(Boolean)
                .join("  —  ")}
            </div>
          )}
        </div>
        <ChevronDown aria-hidden className={cn("size-4 shrink-0 text-subtle transition-transform", open && "rotate-180")} />
      </button>

      {open && (
        <div id={id} className="space-y-4 px-4 pb-4 border-t border-border/50 pt-3">
          {m.description && <p className="text-xs leading-relaxed text-muted">{m.description}</p>}
          
          {/* CostModel & Context Parameters */}
          <div className="rounded-lg bg-hover/40 p-3 space-y-2">
            <div className="flex items-center gap-1.5 text-xs font-medium text-fg">
              <Coins aria-hidden className="size-3.5 text-accent" />
              {t("settings.knowledge.costModel.title")}
            </div>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 text-[11px]">
              <div>
                <span className="text-subtle">{t("settings.knowledge.costModel.cacheRead")}:</span>{" "}
                <span className="font-mono text-fg font-medium">
                  {m.cacheRates ? `${m.cacheRates.readMultiplier}×` : "1.0×"}
                </span>
              </div>
              <div>
                <span className="text-subtle">{t("settings.knowledge.costModel.cacheWrite")}:</span>{" "}
                <span className="font-mono text-fg font-medium">
                  {m.cacheRates ? `${m.cacheRates.writeMultiplier}×` : "1.0×"}
                </span>
              </div>
              {m.maxOutput && (
                <div>
                  <span className="text-subtle">{t("settings.knowledge.costModel.maxOutput")}:</span>{" "}
                  <span className="font-mono text-fg font-medium">{fmtTokens(m.maxOutput)}</span>
                </div>
              )}
              {m.longContextPricing && (
                <div>
                  <span className="text-subtle">{t("settings.knowledge.costModel.longContextThreshold")}:</span>{" "}
                  <span className="font-mono text-fg font-medium">{fmtTokens(m.longContextPricing.threshold)}</span>
                </div>
              )}
            </div>
          </div>

          {facts.length > 0 && (
            <p className="text-[11px] text-subtle">
              {facts.join(" · ")}
              {(m.priceEstimated || m.skillsEstimated) && (
                <span className="ml-2 rounded bg-hover px-1 text-warning">
                  {t(m.skillsEstimated ? "settings.knowledge.models.skillsEstimated" : "settings.knowledge.models.priceEstimated")}
                </span>
              )}
            </p>
          )}

          {/* Domain Skills & Empirical Learning Comparison */}
          {keys.length > 0 ? (
            <div className="space-y-2">
              <div className="flex items-center justify-between text-xs font-medium text-fg">
                <span>{t("settings.knowledge.learning.title")}</span>
                <span className="text-[11px] text-subtle">
                  {t("knowledgeView.priorVsMeasured", { prior: t("settings.knowledge.learning.prior") })}
                  {learnedSamples > 0 ? ` · n=${learnedSamples}` : ` · ${t("knowledgeView.noMeasured")}`}
                </span>
              </div>
              <dl className="grid grid-cols-1 gap-x-6 gap-y-1.5 sm:grid-cols-2">
                {keys.map((d) => (
                  <SkillBar key={d} domain={d} score={m.skills[d]} />
                ))}
              </dl>
            </div>
          ) : (
            <p className="text-xs text-subtle">{t("settings.knowledge.models.noSkills")}</p>
          )}

          {/* Usage Profiles */}
          {Object.keys(usageProfiles).length > 0 && (
            <div className="space-y-1.5">
              <div className="text-xs font-medium text-fg">{t("settings.knowledge.costModel.profiles")}</div>
              <div className="overflow-x-auto">
                <table className="w-full text-left text-[11px]">
                  <thead>
                    <tr className="border-b border-border text-subtle">
                      <th className="py-1 pr-2 font-normal">{t("misc.domainSize")}</th>
                      <th className="py-1 pr-2 font-normal">{t("settings.knowledge.costModel.turns")}</th>
                      <th className="py-1 pr-2 font-normal">{t("settings.knowledge.costModel.calls")}</th>
                      <th className="py-1 pr-2 font-normal">{t("settings.knowledge.costModel.growth")}</th>
                      <th className="py-1 font-normal">{t("settings.knowledge.costModel.output")}</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/40 font-mono">
                    {(["general", "frontend-ui", "backend-api"] as const).map((d) => {
                      const prof = usageProfiles[d];
                      if (!prof) return null;
                      return (["S", "M", "L"] as const).map((sz) => {
                        const sEntry = prof[sz];
                        if (!sEntry) return null;
                        return (
                          <tr key={`${d}-${sz}`} className="text-muted">
                            <td className="py-0.5 pr-2 font-sans font-medium text-fg">{d} ({sz})</td>
                            <td className="py-0.5 pr-2">{sEntry.turns}</td>
                            <td className="py-0.5 pr-2">{sEntry.callsPerTurn}</td>
                            <td className="py-0.5 pr-2">+{fmtTokens(sEntry.growthPerTurn)}</td>
                            <td className="py-0.5">{fmtTokens(sEntry.outputPerTurn)}</td>
                          </tr>
                        );
                      });
                    })}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {m.offerings.length > 0 && <Offerings model={m} providerName={providerName} />}
          <div className="flex items-center justify-between pt-1">
            {m.tags.length > 0 ? (
              <div className="flex flex-wrap gap-1">
                {m.tags.map((tag) => (
                  <span key={tag} className="rounded border border-border px-1.5 text-[10px] text-muted">
                    {tag}
                  </span>
                ))}
              </div>
            ) : <div />}
            {onCopyOverride && (
              <Button size="sm" onClick={() => onCopyOverride(m)}>
                <Sparkles aria-hidden className="size-3 text-accent" />
                {t("settings.knowledge.custom.copyOverride")}
              </Button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/** Names for CLIs not in the last scan (not added yet); mirrors REGISTRY in src-tauri/src/providers.rs. */
const CLI_NAMES: Record<ProviderId, string> = {
  CLAUDE_CODE: "Claude Code",
  CODEX_CLI: "Codex CLI",
  CURSOR_CLI: "Cursor CLI",
  ANTIGRAVITY_CLI: "Antigravity CLI",
  COPILOT_CLI: "GitHub Copilot CLI",
  AIDER: "Aider",
  OPENCODE: "OpenCode",
  GROK_CLI: "Grok Build",
  KIRO_CLI: "Kiro CLI",
  QWEN_CODE: "Qwen Code",
  KIMI_CODE: "Kimi Code",
  QODER_CLI: "Qoder CLI",
  AMP: "Amp",
  CRUSH: "Crush",
  GOOSE: "Goose",
  CLINE: "Cline CLI",
  DROID: "Factory Droid",
  AUGGIE: "Augment Auggie",
  MUSE_CODE: "Muse Code",
  TRAE_CLI: "TraeCode CLI",
  REASONIX: "Reasonix",
};

/** Display name from the last CLI scan, then the whitelist; the pack's provider id otherwise. */
function useProviderName() {
  const providers = useProviders((st) => st.providers);
  return useMemo(() => {
    const names = new Map<string, string>([...Object.entries(CLI_NAMES), ...providers.map((p) => [p.id, p.name] as const)]);
    return (id: string) => names.get(id) ?? id;
  }, [providers]);
}

/** Where a model is available: each CLI's argument and the limits that differ from the model's own. */
function Offerings({ model: m, providerName }: { model: PackModel; providerName: (id: string) => string }) {
  const { t } = useTranslation();
  const limit = (o: PackOffering) =>
    [
      o.contextWindow && t("settings.knowledge.models.context", { value: fmtTokens(o.contextWindow) }),
      o.creditMultiplier !== null && t("settings.knowledge.models.credits", { value: o.creditMultiplier }),
    ]
      .filter(Boolean)
      .join(" · ");
  return (
    <table className="w-full text-left text-xs">
      <caption className="pb-1 text-left text-[11px] font-medium text-muted">{t("settings.knowledge.models.offeredBy")}</caption>
      <thead className="sr-only">
        <tr>
          <th scope="col">{t("settings.knowledge.models.cli")}</th>
          <th scope="col">{t("settings.knowledge.models.arg")}</th>
          <th scope="col">{t("settings.knowledge.models.limits")}</th>
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {m.offerings.map((o) => (
          <tr key={o.provider}>
            <th scope="row" className="py-1 pr-3 font-normal whitespace-nowrap text-fg">
              {providerName(o.provider)}
              {o.billing && <span className="ml-1.5 text-[10px] text-subtle">{t(`settings.knowledge.models.billing.${o.billing}`, o.billing)}</span>}
            </th>
            <td className="py-1 pr-3">
              <code className="font-mono text-[11px] text-muted" data-selectable>
                {o.cliModelArg}
              </code>
              {!o.verified && (
                <span className="ml-1.5 text-[10px] text-warning" title={t("settings.knowledge.models.unverifiedDesc")}>
                  {t("settings.knowledge.models.unverified")}
                </span>
              )}
            </td>
            <td className="py-1 text-right text-[11px] text-subtle">{limit(o)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** One domain weight on the pack's 1–5 scale, with empirical pass rate if available. */
function SkillBar({ domain, score }: { domain: string; score: number }) {
  const pct = ((score - 1) / 4) * 100;
  return (
    <div className="flex items-center gap-2">
      <dt className="w-24 shrink-0 truncate text-xs text-muted" title={domain}>
        {domain}
      </dt>
      <dd className="flex flex-1 items-center gap-2">
        <span
          role="meter"
          aria-label={domain}
          aria-valuemin={1}
          aria-valuemax={5}
          aria-valuenow={score}
          className="h-1.5 flex-1 overflow-hidden rounded-full bg-active"
        >
          <span
            className={cn("block h-full rounded-full", score >= 4.5 ? "bg-accent" : score >= 3 ? "bg-accent/70" : "bg-subtle")}
            style={{ width: `${Math.max(4, pct)}%` }}
          />
        </span>
        <span className="w-6 text-right text-xs text-fg tabular-nums">{+score.toFixed(1)}</span>
      </dd>
    </div>
  );
}
