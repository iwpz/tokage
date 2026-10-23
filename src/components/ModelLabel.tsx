import type { ProviderId } from "../lib/ipc";
import { useModelDisplay } from "../lib/useModelLabel";
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
  const display = useModelDisplay();
  const model = display({ provider, providerName, requestedModel: modelName, details });
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5">
      <ProviderLogo id={provider} name={model.provider} className="size-4 rounded bg-transparent [&>*]:size-3.5" />
      <span className="truncate">{model.label}</span>
    </span>
  );
}
