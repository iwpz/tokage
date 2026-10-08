import { useTranslation } from "react-i18next";
import { ChevronDown, RefreshCw, Settings2 } from "lucide-react";
import { cn } from "../lib/cn";
import type { ProviderStatus } from "../lib/ipc";
import { usePopover } from "../lib/usePopover";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";
import { useUi } from "../stores/ui";
import { CheckingBadge, StatusDot } from "./ProviderBadge";
import { ProviderLogo } from "./ProviderLogo";
import { isProviderShown } from "./settings/CliSettings";

/** Worst-first ordering, so the summary dot reflects the CLI that needs attention. */
const SEVERITY: ProviderStatus[] = ["MELTED", "NEEDS_LOGIN", "DEGRADED", "UNSUPPORTED_VERSION", "UNKNOWN_AUTH", "READY"];

/**
 * Title-bar CLI status: one compact button ("3/4 ready" + summary dot) that opens a list
 * with each installed CLI's actual status. Replaces one pill per provider.
 */
export function ProviderStatusMenu() {
  const { t } = useTranslation();
  const { open, setOpen, ref } = usePopover();
  const { providers, scanning, pending, refresh, healthByProvider, recover, isMelted } =
    useProviders();
  const enabled = useSettings((s) => s.providerEnabled);
  const added = useSettings((s) => s.providerAdded);
  const openSettings = useUi((s) => s.openSettings);

  // Installed CLIs the user hasn't switched off for dispatch.
  const active = providers.filter(
    (p) => p.status !== "NOT_INSTALLED" && enabled[p.id] !== false && isProviderShown(p, added),
  );
  const ready = active.filter((p) => p.status === "READY" && !isMelted(p.id)).length;
  const anyMelted = active.some((p) => isMelted(p.id));
  const summary =
    (anyMelted ? "MELTED" : SEVERITY.find((s) => active.some((p) => p.status === s))) ??
    "NOT_INSTALLED";

  return (
    <span ref={ref} className="relative inline-flex">
      <button
        type="button"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "inline-flex h-7 items-center gap-1.5 rounded-full border border-border px-2.5 text-xs text-muted hover:bg-hover hover:text-fg",
          open && "bg-hover text-fg",
        )}
      >
        <StatusDot status={summary} />
        {active.length === 0
          ? t("topbar.noProviders")
          : t("topbar.cliSummary", { ready, total: active.length })}
        <ChevronDown aria-hidden className="size-3 text-subtle" />
      </button>

      {open && (
        <div
          role="dialog"
          aria-label={t("topbar.cliPanel")}
          className="absolute right-0 top-full z-30 mt-1.5 w-72 rounded-lg border border-border bg-elevated p-1 text-[13px] shadow-lg"
        >
          {active.length === 0 ? (
            <p className="px-2 py-3 text-xs text-subtle">{t("topbar.noProvidersHint")}</p>
          ) : (
            <ul>
              {active.map((p) => (
                <li key={p.id}>
                  <button
                    type="button"
                    onClick={() => {
                      setOpen(false);
                      openSettings("cli");
                    }}
                    className="flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left hover:bg-hover"
                  >
                    <ProviderLogo id={p.id} name={p.name} className="size-7" />
                    <span className="min-w-0 flex-1">
                      {/* Version shares the name row so long builds (e.g. Cursor) can't squeeze the status line. */}
                      <span className="flex items-baseline gap-2">
                        <span className="min-w-0 flex-1 truncate text-fg">{p.name}</span>
                        {p.version && (
                          <span
                            title={p.version}
                            className="min-w-0 max-w-[55%] shrink truncate font-mono text-[11px] text-subtle"
                          >
                            {p.version}
                          </span>
                        )}
                      </span>
                      <span className="flex items-center gap-2 whitespace-nowrap">
                        <StatusDot
                          status={isMelted(p.id) ? "MELTED" : p.status}
                          withLabel
                        />
                        {pending.includes(p.id) && <CheckingBadge />}
                      </span>
                      {isMelted(p.id) && healthByProvider[p.id] && (
                        <span className="mt-0.5 block text-[11px] text-danger">
                          {t("topbar.meltedReason", {
                            reason: healthByProvider[p.id].meltReason ?? "—",
                            until: healthByProvider[p.id].meltedUntil
                              ? t("topbar.meltedUntil", {
                                  until: healthByProvider[p.id].meltedUntil,
                                })
                              : "",
                          })}
                          <button
                            type="button"
                            className="ml-2 underline hover:text-fg"
                            onClick={(ev) => {
                              ev.stopPropagation();
                              void recover(p.id);
                            }}
                          >
                            {t("topbar.recoverProvider")}
                          </button>
                        </span>
                      )}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="mt-1 flex items-center gap-1 border-t border-border pt-1">
            <button
              type="button"
              onClick={() => void refresh()}
              disabled={scanning}
              className="flex h-8 flex-1 items-center gap-2 rounded-md px-2 text-muted hover:bg-hover hover:text-fg disabled:opacity-50"
            >
              <RefreshCw aria-hidden className={cn("size-3.5", scanning && "animate-spin")} />
              {scanning ? t("settings.cli.scanning") : t("settings.cli.refresh")}
            </button>
            <button
              type="button"
              onClick={() => {
                setOpen(false);
                openSettings("cli");
              }}
              className="flex h-8 flex-1 items-center gap-2 rounded-md px-2 text-muted hover:bg-hover hover:text-fg"
            >
              <Settings2 aria-hidden className="size-3.5" />
              {t("topbar.manageClis")}
            </button>
          </div>
        </div>
      )}
    </span>
  );
}
