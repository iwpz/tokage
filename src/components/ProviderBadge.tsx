import { useTranslation } from "react-i18next";
import { cn } from "../lib/cn";
import type { ProviderStatus } from "../lib/ipc";

const TONE: Record<ProviderStatus | "DISABLED", string> = {
  READY: "bg-success",
  NOT_INSTALLED: "bg-subtle",
  NEEDS_LOGIN: "bg-warning",
  UNSUPPORTED_VERSION: "bg-warning",
  UNKNOWN_AUTH: "bg-warning",
  DEGRADED: "bg-warning",
  MELTED: "bg-danger",
  DISABLED: "bg-subtle",
};

/** Status dot plus text, so state is never conveyed by color alone (docs/10 §6). */
export function StatusDot({ status, withLabel }: { status: ProviderStatus | "DISABLED"; withLabel?: boolean }) {
  const { t } = useTranslation();
  return (
    <span className="inline-flex items-center gap-1.5">
      <span aria-hidden className={cn("size-1.5 rounded-full", TONE[status])} />
      {withLabel ? (
        <span className="text-xs text-muted">{t(`status.${status}`)}</span>
      ) : (
        <span className="sr-only">{t(`status.${status}`)}</span>
      )}
    </span>
  );
}

/** Small "re-checking" marker shown while a cached row waits for its fresh scan result. */
export function CheckingBadge() {
  const { t } = useTranslation();
  return (
    <span role="status" className="inline-flex items-center gap-1 text-[11px] text-subtle">
      <span aria-hidden className="size-2.5 animate-spin rounded-full border border-current border-t-transparent" />
      {t("settings.cli.checking")}
    </span>
  );
}
