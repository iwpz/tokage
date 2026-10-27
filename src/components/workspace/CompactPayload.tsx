import { useTranslation } from "react-i18next";

/** Prepared input, not evidence that the provider applied compaction. */
export function CompactPayload({payload}:{payload?:string}) {
  const {t} = useTranslation();
  if (!payload) return null;
  return <details className="mb-2 rounded border border-border p-2 text-xs text-subtle"><summary className="cursor-pointer">{t("workspace.context.preparedPayload")}</summary><pre className="mt-2 whitespace-pre-wrap break-words font-mono">{payload}</pre></details>;
}
