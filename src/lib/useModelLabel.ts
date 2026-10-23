import { useCallback } from "react";
import { modelDisplay, type ModelDisplayInput } from "./modelDisplay";
import { parseSpec } from "./modelSpec";
import { useProviders } from "../stores/providers";

export function useModelDisplay() {
  const providers = useProviders(state => state.providers);
  return useCallback((input: ModelDisplayInput | string) => {
    if (typeof input !== "string") return modelDisplay(input, providers);
    const { provider, model } = parseSpec(input);
    return modelDisplay({ provider, requestedModel: model }, providers);
  }, [providers]);
}

export function useModelLabel() {
  const display = useModelDisplay();
  return useCallback((input: ModelDisplayInput | string) => display(input).label, [display]);
}
