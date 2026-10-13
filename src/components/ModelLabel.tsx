import type { ProviderId } from "../lib/ipc";
import { vendorLabel } from "../lib/models";
import { ProviderLogo } from "./ProviderLogo";

/** Shared presentation for selected and assigned models. */
export function ModelLabel({
  provider,
  providerName,
  modelName,
  details = [],
}: {
  provider: ProviderId;
  providerName?: string;
  modelName: string;
  details?: string[];
}) {
  const vendor = vendorLabel(provider, providerName);
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5">
      <ProviderLogo id={provider} name={vendor} className="size-4 rounded bg-transparent [&>*]:size-3.5" />
      <span className="truncate">{[vendor, modelName, ...details].join(" · ")}</span>
    </span>
  );
}
