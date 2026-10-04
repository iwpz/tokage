import { useTranslation } from "react-i18next";
import { TriangleAlert } from "lucide-react";
import { inTauri } from "../lib/ipc";
import { retryImport, useStorageStatus } from "../lib/db";
import { Button } from "./ui";

/**
 * Shown when the database couldn't be opened or the localStorage import failed (P1.m3d).
 * Tokage then keeps working on the old data, which stays untouched until a retry succeeds.
 */
export function StorageBanner() {
  const { t } = useTranslation();
  const { mode, error, retrying } = useStorageStatus();
  if (!inTauri || mode !== "legacy") return null;
  return (
    <div role="alert" className="flex items-center gap-3 border-b border-border bg-hover px-4 py-2 text-xs">
      <TriangleAlert className="size-4 shrink-0 text-danger" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="text-fg">{t("storage.failed")}</p>
        {error && <p className="truncate text-subtle" title={error}>{error}</p>}
      </div>
      <Button size="sm" disabled={retrying} onClick={() => void retryImport()}>
        {retrying ? t("storage.retrying") : t("storage.retry")}
      </Button>
    </div>
  );
}
