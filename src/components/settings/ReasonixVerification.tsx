import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { ModelVerification } from "../../lib/ipc";
import { Button } from "../ui";

/** Model authorization and the independently queried wallet carry different evidence. */
export function ReasonixVerification({ proof, models, busy, disabled, error, onVerify }: {
  proof?: ModelVerification | null;
  models: string[];
  busy: boolean;
  disabled: boolean;
  error?: string;
  onVerify: (model: string) => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const [selected, setSelected] = useState(proof?.model ?? "");
  const model = models.includes(selected) ? selected : "";
  const state = proof?.state === "VALID" && (proof.expiresAt ?? 0) <= Date.now() / 1000 ? "EXPIRED" : proof?.state ?? "REQUIRED";
  return <div className="space-y-2 text-xs">
    <p role="status">{t(`settings.cli.modelVerification.${state}`)}</p>
    {proof?.model && <p className="break-all text-subtle">{t("settings.cli.modelVerification.scope", { model: proof.model })}</p>}
    {proof?.verifiedAt && proof.expiresAt && <p className="text-subtle">{t("settings.cli.modelVerification.time", {
      verified: new Date(proof.verifiedAt * 1000).toLocaleString(i18n.language),
      expires: new Date(proof.expiresAt * 1000).toLocaleString(i18n.language),
    })}</p>}
    <p className="text-subtle">{t("settings.cli.modelVerification.walletSeparate")}</p>
    <div className="flex flex-wrap items-center gap-2">
      <select aria-label={t("settings.cli.modelVerification.select")} value={model}
        disabled={disabled || busy} onChange={(e) => setSelected(e.target.value)}
        className="h-8 min-w-0 max-w-full rounded-md border border-border bg-elevated px-2 text-xs">
        <option value="">{t("settings.cli.modelVerification.select")}</option>
        {models.map((id) => <option key={id} value={id}>{id}</option>)}
      </select>
      <Button variant="ghost" className="h-8 px-2 text-xs" disabled={disabled || busy || !model}
        onClick={() => void onVerify(model)}>{t(busy ? "settings.cli.modelVerification.verifying" : "settings.cli.modelVerification.verify")}</Button>
    </div>
    <p className="text-subtle">{t("settings.cli.modelVerification.explicit")}</p>
    {error && <p role="alert" className="break-words text-warning">{error}</p>}
  </div>;
}
