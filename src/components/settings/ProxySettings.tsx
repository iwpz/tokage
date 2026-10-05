import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, CheckCircle2, ChevronDown, ChevronRight, Loader2, RefreshCw, XCircle } from "lucide-react";
import { cn } from "../../lib/cn";
import {
  getAllProviderProxyStatuses,
  getProxyConfig,
  getSystemProxy,
  saveProxyConfig,
  testProxyConnection,
  type GlobalProxyConfig,
  type ProviderId,
  type ProviderProxyStatus,
  type ProxyMode,
  type ProxyProtocol,
  type ProxyTestResult,
  type ProxyUpstream,
} from "../../lib/ipc";
import { useProviders } from "../../stores/providers";
import { ProviderLogo } from "../ProviderLogo";
import { Button, Card, Select, SettingRow, Switch } from "../ui";

const GLOBAL_MODES: ProxyMode[] = ["cli_own", "none", "system", "manual"];
const CLI_MODES: ProxyMode[] = ["inherit_global", "cli_own", "none", "system", "manual"];
const PROTOCOLS: ProxyProtocol[] = ["http", "https", "socks5"];

export function ProxySettings() {
  const { t } = useTranslation();
  const allProviders = useProviders((p) => p.providers);
  // Only list CLIs that are installed, ready/degraded, and have models (docs/12 §5)
  const eligibleProviders = allProviders.filter(
    (p) => (p.status === "READY" || p.status === "DEGRADED") && p.models.length > 0
  );

  const [config, setConfig] = useState<GlobalProxyConfig>({
    mode: "cli_own",
    upstream: null,
    no_proxy: "localhost,127.0.0.1,::1",
    ca_file: null,
    local_relay: false,
    per_cli: {},
  });
  const [systemProxy, setSystemProxy] = useState<{
    upstream: ProxyUpstream | null;
    noProxy: string | null;
  } | null>(null);
  const [providerStatuses, setProviderStatuses] = useState<Record<string, ProviderProxyStatus>>({});
  const [loading, setLoading] = useState(true);
  const [saveSuccess, setSaveSuccess] = useState(false);

  // Global test state
  const [globalTesting, setGlobalTesting] = useState(false);
  const [globalTestResult, setGlobalTestResult] = useState<ProxyTestResult | null>(null);

  // Per-CLI test state
  const [cliTesting, setCliTesting] = useState<Record<string, boolean>>({});
  const [cliTestResults, setCliTestResults] = useState<Record<string, ProxyTestResult>>({});

  // Expanded details per CLI
  const [expandedCli, setExpandedCli] = useState<Record<string, boolean>>({});

  useEffect(() => {
    async function load() {
      setLoading(true);
      try {
        const [cfg, sys, statuses] = await Promise.all([
          getProxyConfig(),
          getSystemProxy(),
          getAllProviderProxyStatuses(),
        ]);
        setConfig(cfg);
        setSystemProxy(sys);
        const map: Record<string, ProviderProxyStatus> = {};
        for (const s of statuses) {
          map[s.provider] = s;
        }
        setProviderStatuses(map);
      } catch (err) {
        console.error("Failed to load proxy settings:", err);
      } finally {
        setLoading(false);
      }
    }
    load();
  }, []);

  const handleSave = async (newConfig: GlobalProxyConfig) => {
    setConfig(newConfig);
    setSaveSuccess(false);
    try {
      await saveProxyConfig(newConfig);
      setSaveSuccess(true);
      setTimeout(() => setSaveSuccess(false), 2000);
    } catch (err) {
      console.error("Failed to save proxy config:", err);
    }
  };

  const handleGlobalTest = async () => {
    setGlobalTesting(true);
    setGlobalTestResult(null);
    try {
      const res = await testProxyConnection(undefined, undefined, config);
      setGlobalTestResult(res);
    } catch (err) {
      setGlobalTestResult({
        success: false,
        endpoint: "https://api.openai.com",
        latency_ms: 0,
        message: String(err),
      });
    } finally {
      setGlobalTesting(false);
    }
  };

  const handleCliTest = async (providerId: ProviderId) => {
    setCliTesting((prev) => ({ ...prev, [providerId]: true }));
    setCliTestResults((prev) => {
      const next = { ...prev };
      delete next[providerId];
      return next;
    });
    try {
      const res = await testProxyConnection(providerId, undefined, config);
      setCliTestResults((prev) => ({ ...prev, [providerId]: res }));
    } catch (err) {
      setCliTestResults((prev) => ({
        ...prev,
        [providerId]: {
          success: false,
          endpoint: "",
          latency_ms: 0,
          message: String(err),
        },
      }));
    } finally {
      setCliTesting((prev) => ({ ...prev, [providerId]: false }));
    }
  };

  if (loading) {
    return (
      <div className="flex h-48 items-center justify-center">
        <Loader2 className="size-6 animate-spin text-muted" />
      </div>
    );
  }

  const globalModeOptions = GLOBAL_MODES.map((m) => ({
    value: m,
    label: t(`settings.proxy.modes.${m}`),
  }));

  const cliModeOptions = CLI_MODES.map((m) => ({
    value: m,
    label: t(`settings.proxy.modes.${m}`),
  }));

  const isWindows =
    typeof navigator !== "undefined" &&
    (/win(dows|32|64)/i.test(navigator.userAgent) || (navigator as any).userAgentData?.platform === "Windows");

  return (
    <div className="space-y-6">
      {/* 1. Global Proxy Settings Card */}
      <Card>
        <div className="px-4 pt-3.5 pb-2">
          <h2 className="text-[13px] font-medium text-fg">
            {t("settings.proxy.globalTitle")}
          </h2>
        </div>

        <SettingRow
          title={t("settings.proxy.globalModeLabel")}
          description={t("settings.proxy.globalModeDesc")}
        >
          <div className="flex items-center gap-3">
            <Select
              label={t("settings.proxy.globalModeLabel")}
              value={config.mode}
              options={globalModeOptions}
              onChange={(val) => handleSave({ ...config, mode: val as ProxyMode })}
            />
          </div>
        </SettingRow>

        {/* System proxy detected banner */}
        {systemProxy && (
          <div className="px-4 py-3">
            <div className="rounded-lg border border-border/70 bg-hover/40 px-3.5 py-2.5 text-xs text-muted">
              <span className="font-medium text-fg">
                {t("settings.proxy.systemDetected")}:{" "}
              </span>
              {systemProxy.upstream ? (
                <span className="font-mono text-fg">
                  {systemProxy.upstream.protocol}://{systemProxy.upstream.host}:{systemProxy.upstream.port}
                </span>
              ) : (
                <span>{t("settings.proxy.systemNone")}</span>
              )}
              {systemProxy.noProxy && (
                <span className="ml-2 font-mono">
                  (NO_PROXY: {systemProxy.noProxy})
                </span>
              )}
            </div>
          </div>
        )}

        {/* Upstream Proxy Configuration (for manual mode or fallback) */}
        {config.mode === "manual" && (
          <div className="px-4 py-3">
            <div className="space-y-3 rounded-lg border border-border/80 bg-elevated/50 p-4">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold uppercase tracking-wider text-muted">
                  {t("settings.proxy.upstreamConfig")}
                </span>
              <Button
                variant="secondary"
                size="sm"
                disabled={globalTesting}
                onClick={handleGlobalTest}
              >
                {globalTesting ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <RefreshCw className="size-3.5" />
                )}
                {t("settings.proxy.testConnect")}
              </Button>
            </div>

            {globalTestResult && (
              <div
                className={cn(
                  "flex items-center gap-2 rounded-md p-2 text-xs",
                  globalTestResult.success
                    ? "bg-green-500/10 text-green-600 dark:text-green-400 border border-green-500/20"
                    : "bg-red-500/10 text-red-600 dark:text-red-400 border border-red-500/20"
                )}
              >
                {globalTestResult.success ? (
                  <CheckCircle2 className="size-4 shrink-0" />
                ) : (
                  <XCircle className="size-4 shrink-0" />
                )}
                <span>
                  {globalTestResult.endpoint} — {globalTestResult.latency_ms}ms
                  {globalTestResult.message ? ` (${globalTestResult.message})` : ""}
                </span>
              </div>
            )}

            <div className="grid grid-cols-12 gap-2">
              <div className="col-span-3">
                <label className="text-[11px] text-muted">
                  {t("settings.proxy.protocol")}
                </label>
                <select
                  value={config.upstream?.protocol || "http"}
                  onChange={(e) => {
                    const protocol = e.target.value as ProxyProtocol;
                    const up: ProxyUpstream = {
                      protocol,
                      host: config.upstream?.host || "127.0.0.1",
                      port: config.upstream?.port || 7890,
                      username: config.upstream?.username,
                      password: config.upstream?.password,
                    };
                    handleSave({ ...config, upstream: up });
                  }}
                  className="mt-1 h-8 w-full rounded-md border border-border bg-elevated px-2 text-xs text-fg"
                >
                  {PROTOCOLS.map((p) => (
                    <option key={p} value={p}>
                      {p.toUpperCase()}
                    </option>
                  ))}
                </select>
              </div>

              <div className="col-span-6">
                <label className="text-[11px] text-muted">
                  {t("settings.proxy.host")}
                </label>
                <input
                  type="text"
                  placeholder="127.0.0.1"
                  value={config.upstream?.host || ""}
                  onChange={(e) => {
                    const host = e.target.value;
                    const up: ProxyUpstream = {
                      protocol: config.upstream?.protocol || "http",
                      host,
                      port: config.upstream?.port || 7890,
                      username: config.upstream?.username,
                      password: config.upstream?.password,
                    };
                    handleSave({ ...config, upstream: up });
                  }}
                  className="mt-1 h-8 w-full rounded-md border border-border bg-elevated px-2 text-xs font-mono text-fg"
                />
              </div>

              <div className="col-span-3">
                <label className="text-[11px] text-muted">
                  {t("settings.proxy.port")}
                </label>
                <input
                  type="number"
                  placeholder="7890"
                  value={config.upstream?.port || ""}
                  onChange={(e) => {
                    const port = parseInt(e.target.value, 10) || 0;
                    const up: ProxyUpstream = {
                      protocol: config.upstream?.protocol || "http",
                      host: config.upstream?.host || "127.0.0.1",
                      port,
                      username: config.upstream?.username,
                      password: config.upstream?.password,
                    };
                    handleSave({ ...config, upstream: up });
                  }}
                  className="mt-1 h-8 w-full rounded-md border border-border bg-elevated px-2 text-xs font-mono text-fg"
                />
              </div>
            </div>

            <div className="grid grid-cols-2 gap-2 pt-1">
              <div>
                <label className="text-[11px] text-muted">
                  {t("settings.proxy.authUsername")}
                </label>
                <input
                  type="text"
                  placeholder="Username (optional)"
                  value={config.upstream?.username || ""}
                  onChange={(e) => {
                    const username = e.target.value || null;
                    const up: ProxyUpstream = {
                      protocol: config.upstream?.protocol || "http",
                      host: config.upstream?.host || "127.0.0.1",
                      port: config.upstream?.port || 7890,
                      username,
                      password: config.upstream?.password,
                    };
                    handleSave({ ...config, upstream: up });
                  }}
                  className="mt-1 h-8 w-full rounded-md border border-border bg-elevated px-2 text-xs text-fg"
                />
              </div>
              <div>
                <label className="text-[11px] text-muted">
                  {t("settings.proxy.authPassword")}
                </label>
                <input
                  type="password"
                  placeholder="Password (optional)"
                  value={config.upstream?.password || ""}
                  onChange={(e) => {
                    const password = e.target.value || null;
                    const up: ProxyUpstream = {
                      protocol: config.upstream?.protocol || "http",
                      host: config.upstream?.host || "127.0.0.1",
                      port: config.upstream?.port || 7890,
                      username: config.upstream?.username,
                      password,
                    };
                    handleSave({ ...config, upstream: up });
                  }}
                  className="mt-1 h-8 w-full rounded-md border border-border bg-elevated px-2 text-xs text-fg"
                />
              </div>
            </div>
            <p className="text-[11px] text-muted/80">
              {t("settings.proxy.authHint")}
            </p>
          </div>
        </div>
      )}

        {/* NO_PROXY Input */}
        <SettingRow
          title={t("settings.proxy.noProxyLabel")}
          description={t("settings.proxy.noProxyDesc")}
        >
          <input
            type="text"
            value={config.no_proxy}
            onChange={(e) => handleSave({ ...config, no_proxy: e.target.value })}
            className="h-8 w-64 rounded-md border border-border bg-elevated px-2 text-xs font-mono text-fg"
          />
        </SettingRow>

        {/* CA Certificate File */}
        <SettingRow
          title={t("settings.proxy.caCertLabel")}
          description={t("settings.proxy.caCertDesc")}
        >
          <input
            type="text"
            placeholder="/path/to/enterprise-ca.pem"
            value={config.ca_file || ""}
            onChange={(e) =>
              handleSave({
                ...config,
                ca_file: e.target.value.trim() ? e.target.value : null,
              })
            }
            className="h-8 w-64 rounded-md border border-border bg-elevated px-2 text-xs font-mono text-fg"
          />
        </SettingRow>

        {/* Local CONNECT Relay Toggle */}
        <SettingRow
          title={t("settings.proxy.localRelayLabel")}
          description={t("settings.proxy.localRelayDesc")}
        >
          <Switch
            checked={config.local_relay}
            onChange={(checked) => handleSave({ ...config, local_relay: checked })}
            label={t("settings.proxy.localRelayLabel")}
          />
        </SettingRow>

        <div className="flex items-center justify-between px-4 py-3 text-xs text-muted">
          <span>{t("settings.proxy.turnNotice")}</span>
          {saveSuccess && (
            <span className="flex items-center gap-1 text-green-500">
              <Check className="size-3.5" />
              {t("settings.proxy.saved")}
            </span>
          )}
        </div>
      </Card>

      {/* 2. Per-CLI Proxy Settings & Status */}
      <Card>
        <div className="px-4 pt-3.5 pb-2">
          <h2 className="text-[13px] font-medium text-fg">
            {t("settings.proxy.perCliTitle")}
          </h2>
          <p className="mt-1 text-xs text-muted">
            {t("settings.proxy.perCliDesc")}
          </p>
        </div>

        {eligibleProviders.length === 0 ? (
          <div className="px-4 py-6 text-center text-xs text-muted">
            {t("settings.proxy.noEligibleClis")}
          </div>
        ) : (
          <div className="divide-y divide-border/60">
            {eligibleProviders.map((provider) => {
              const status = providerStatuses[provider.id];
              const cliCfg = config.per_cli[provider.id] || {
                mode: "inherit_global",
                forced: false,
              };
              const isExpanded = !!expandedCli[provider.id];
              const isTesting = !!cliTesting[provider.id];
              const testResult = cliTestResults[provider.id];
              const forcedSupported = status ? status.forced_supported && !isWindows : false;

              return (
                <div key={provider.id} className="px-4 py-3.5 space-y-2.5">
                  {/* Row 1: CLI Identification & Details Toggle */}
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-2.5 min-w-0">
                      <ProviderLogo id={provider.id} name={provider.name} className="size-7 shrink-0" />
                      <div className="flex items-center gap-2 min-w-0">
                        <span className="text-[13px] font-medium text-fg truncate">
                          {provider.name}
                        </span>
                        {provider.version && (
                          <span className="shrink-0 font-mono text-[11px] text-muted">
                            v{provider.version}
                          </span>
                        )}
                        {status?.verified && (
                          <span className="shrink-0 whitespace-nowrap rounded bg-accent/10 px-1.5 py-0.5 text-[9px] font-medium leading-none text-accent">
                            {t("settings.proxy.verifiedBadge")}
                          </span>
                        )}
                      </div>
                    </div>

                    {/* Details expand/collapse */}
                    <button
                      type="button"
                      onClick={() =>
                        setExpandedCli((prev) => ({
                          ...prev,
                          [provider.id]: !isExpanded,
                        }))
                      }
                      className="flex items-center gap-1 rounded px-1.5 py-0.5 text-xs text-muted hover:bg-hover hover:text-fg shrink-0 transition-colors"
                      aria-label={t("settings.proxy.details")}
                    >
                      <span className="text-[11px]">{t("settings.proxy.details")}</span>
                      {isExpanded ? (
                        <ChevronDown className="size-3.5" />
                      ) : (
                        <ChevronRight className="size-3.5" />
                      )}
                    </button>
                  </div>

                  {/* Warning if own proxy config detected */}
                  {status?.own_config_proxy && (
                    <div className="rounded border border-amber-500/20 bg-amber-500/10 px-2.5 py-1 text-[11px] text-amber-600 dark:text-amber-400">
                      <span className="font-medium">{t("settings.proxy.ownConfigDetected")}:</span>{" "}
                      <span className="font-mono">{status.own_config_proxy}</span>
                    </div>
                  )}

                  {/* Row 2: Controls Toolbar */}
                  <div className="flex flex-wrap items-center justify-between gap-2.5 rounded-lg border border-border/60 bg-hover/30 px-3 py-2 text-xs">
                    {/* Mode Dropdown */}
                    <div className="flex items-center gap-2">
                      <span className="text-muted shrink-0 text-xs">
                        {t("settings.proxy.cliModeLabel")}:
                      </span>
                      <Select
                        label={provider.name}
                        value={cliCfg.mode}
                        options={cliModeOptions}
                        className="h-7.5 w-32 min-w-0 text-xs px-2"
                        onChange={(val) => {
                          const updated = {
                            ...config,
                            per_cli: {
                              ...config.per_cli,
                              [provider.id]: {
                                ...cliCfg,
                                mode: val as ProxyMode,
                              },
                            },
                          };
                          handleSave(updated);
                        }}
                      />
                    </div>

                    {/* Right Controls: Forced Switch & Test Connection */}
                    <div className="flex items-center gap-3 shrink-0">
                      {/* Forced Mode Toggle */}
                      <div
                        className="flex items-center gap-1.5"
                        title={
                          !forcedSupported
                            ? isWindows
                              ? t("settings.proxy.forcedWindowsUnsupported")
                              : t("settings.proxy.forcedNotSupported")
                            : undefined
                        }
                      >
                        <span
                          className={cn(
                            "text-xs shrink-0 select-none",
                            !forcedSupported ? "text-muted/50" : "text-muted"
                          )}
                        >
                          {t("settings.proxy.forcedLabel")}
                        </span>
                        <Switch
                          checked={cliCfg.forced && forcedSupported}
                          onChange={(checked) => {
                            if (!forcedSupported) return;
                            const updated = {
                              ...config,
                              per_cli: {
                                ...config.per_cli,
                                [provider.id]: {
                                  ...cliCfg,
                                  forced: checked,
                                },
                              },
                            };
                            handleSave(updated);
                          }}
                          label={t("settings.proxy.forcedLabel")}
                        />
                      </div>

                      {/* Test Connection Button */}
                      <Button
                        variant="secondary"
                        size="sm"
                        disabled={isTesting}
                        onClick={() => handleCliTest(provider.id)}
                        className="h-7.5 px-2.5 text-xs shrink-0 gap-1.5"
                      >
                        {isTesting ? (
                          <Loader2 className="size-3.5 animate-spin" />
                        ) : (
                          <RefreshCw className="size-3.5" />
                        )}
                        {t("settings.proxy.testConnect")}
                      </Button>
                    </div>
                  </div>

                  {/* Test Result Message */}
                  {testResult && (
                    <div
                      className={cn(
                        "flex items-center justify-between rounded-md px-2.5 py-1.5 text-xs",
                        testResult.success
                          ? "bg-green-500/10 text-green-600 dark:text-green-400 border border-green-500/20"
                          : "bg-red-500/10 text-red-600 dark:text-red-400 border border-red-500/20"
                      )}
                    >
                      <div className="flex items-center gap-2 truncate">
                        {testResult.success ? (
                          <CheckCircle2 className="size-3.5 shrink-0" />
                        ) : (
                          <XCircle className="size-3.5 shrink-0" />
                        )}
                        <span className="truncate">{testResult.endpoint}</span>
                      </div>
                      <span className="font-mono shrink-0 ml-2">
                        {testResult.latency_ms}ms
                        {testResult.message ? ` (${testResult.message})` : ""}
                      </span>
                    </div>
                  )}

                  {/* Expanded Capability & Configuration Details */}
                  {isExpanded && status && (
                    <div className="rounded-lg border border-border/50 bg-hover/30 p-3 text-xs space-y-1.5">
                      <div className="flex items-center justify-between">
                        <span className="text-muted">{t("settings.proxy.recognizedVars")}:</span>
                        <span className="font-mono text-fg">
                          {status.env_vars.join(", ") || t("settings.proxy.none")}
                        </span>
                      </div>
                      {Object.keys(status.extra_env).length > 0 && (
                        <div className="flex items-center justify-between">
                          <span className="text-muted">{t("settings.proxy.extraEnv")}:</span>
                          <span className="font-mono text-fg">
                            {Object.entries(status.extra_env)
                              .map(([k, v]) => `${k}=${v}`)
                              .join(" ")}
                          </span>
                        </div>
                      )}
                      <div className="flex items-center justify-between">
                        <span className="text-muted">{t("settings.proxy.socksSupport")}:</span>
                        <span className="text-fg">
                          {status.supports_socks
                            ? t("settings.proxy.supported")
                            : t("settings.proxy.unsupportedSocksViaRelay")}
                        </span>
                      </div>
                      <div className="flex items-center justify-between">
                        <span className="text-muted">{t("settings.proxy.ownConfigOverride")}:</span>
                        <span className="text-fg">
                          {status.own_config_proxy
                            ? status.own_config_proxy
                            : t("settings.proxy.noOwnConfig")}
                        </span>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </Card>
    </div>
  );
}
