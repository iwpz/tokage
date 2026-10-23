import { useModelLabel } from "../../lib/useModelLabel";
import { builtinDebugCatalog } from "../../lib/debugProvider";
import { DEMO_PROJECT_ID } from "../../lib/demoIdentity";
import { useProjects } from "../../stores/projects";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ModelPicker } from "../ModelPicker";
import type { ModelParams } from "../../lib/ipc";
import { findModel, refKey, resolveVariant, selectableGroups, type ModelOptions, type ModelRef } from "../../lib/models";
import { formatSpec, locateSpec, optionsOf, paramsOf } from "../../lib/modelSpec";
import { useProviders } from "../../stores/providers";
import { useSettings } from "../../stores/settings";

/** What the picker hands back: the concrete id to run and the flags the id can't carry. */
export interface TaskModelChoice {
  spec: string;
  params: ModelParams | null;
}

/**
 * Per-task model picker. It is the composer's `ModelPicker` fed with the same runnable
 * catalog (`selectableGroups`), so a task can only be given a model that can actually run,
 * with the same effort / context / thinking / fast choices, and what it emits is always a
 * concrete catalog id (`resolveVariant`), never a group's first variant.
 */
export function TaskModelPicker({
  value,
  params,
  placeholder,
  onChange,
  onReset,
}: {
  value?: string | null;
  params?: ModelParams | null;
  placeholder: string;
  onChange: (choice: TaskModelChoice) => void;
  onReset: () => void;
}) {
  const { t } = useTranslation();
  const modelLabel = useModelLabel();
  const isDemo = useProjects(s => s.activeProjectId === DEMO_PROJECT_ID);
  const providers = useProviders((s) => s.providers);
  const providerEnabled = useSettings((s) => s.providerEnabled);
  const providerTrust = useSettings((s) => s.providerTrust);
  const blockedModels = useSettings((s) => s.blockedModels);
  const banningLevel = useSettings((s) => s.modelBanningLevel);
  const prefs = useSettings((s) => s.modelProviderPreferences);
  const access = useSettings((s) => s.modelChannelAccess);
  const customModels = useSettings((s) => s.customModels);

  const groups = useMemo(
    () =>
      isDemo ? selectableGroups(builtinDebugCatalog(), {}, {}, "direct") : selectableGroups(providers, providerEnabled, providerTrust, "direct", blockedModels, banningLevel, prefs, access, customModels),
    [isDemo, providers, providerEnabled, providerTrust, blockedModels, banningLevel, prefs, access, customModels],
  );
  const picked = useMemo(() => locateSpec(groups, value), [groups, value]);

  // Options per model; the ref mirrors the state so a pick made right after an option change
  // (the picker fires both) sees the new options.
  const [options, setOptions] = useState<Record<string, ModelOptions>>({});
  const optionsRef = useRef(options);
  const setOptionsFor = useCallback((ref: ModelRef, o: ModelOptions) => {
    optionsRef.current = { ...optionsRef.current, [refKey(ref)]: o };
    setOptions(optionsRef.current);
  }, []);

  // Show the stored choice's options, not defaults.
  const paramsKey = JSON.stringify(params ?? null);
  useEffect(() => {
    if (picked) setOptionsFor(picked.ref, optionsOf(picked.model, picked.variant, params));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [picked?.ref.provider, picked?.ref.model, picked?.variant.id, paramsKey]);

  const emit = (ref: ModelRef, o: ModelOptions) => {
    const model = findModel(groups, ref);
    if (!model) return;
    const r = resolveVariant(model, o);
    onChange({ spec: formatSpec(ref.provider, r.id), params: paramsOf(r) });
  };

  return (
    <ModelPicker
      compact
      placement="down"
      groups={groups}
      value={picked?.ref ?? null}
      recommended={null}
      overridden={!!value}
      // A stored id that isn't runnable now (blocked, signed out, removed) is still shown.
      placeholder={value && !picked ? modelLabel(value) : placeholder}
      onChange={(ref) => emit(ref, optionsRef.current[refKey(ref)] ?? {})}
      onReset={onReset}
      options={options}
      onOptionsChange={(ref, o) => {
        setOptionsFor(ref, o);
        emit(ref, o);
      }}
      label={t("plan.tasks.picker.title")}
    />
  );
}
