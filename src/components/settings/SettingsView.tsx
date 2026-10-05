import type { ComponentType } from "react";
import { useTranslation } from "react-i18next";
import { useUi } from "../../stores/ui";
import { GeneralSettings } from "./GeneralSettings";
import { CliSettings, CliSettingsActions } from "./CliSettings";
import {
  AboutSettings,
  BudgetSettings,
  ContextSettings,
  ExecutionSettings,
  PlanningSettings,
} from "./OtherSettings";
import { KnowledgeSettings } from "./KnowledgeSettings";
import { SecuritySettings } from "./SecuritySettings";
import { ProxySettings } from "./ProxySettings";

const SECTIONS = {
  general: GeneralSettings,
  cli: CliSettings,
  planning: PlanningSettings,
  context: ContextSettings,
  budget: BudgetSettings,
  execution: ExecutionSettings,
  security: SecuritySettings,
  proxy: ProxySettings,
  knowledge: KnowledgeSettings,
  about: AboutSettings,
};

/** Optional controls pinned in the sticky section header (e.g. Rescan). */
const ACTIONS: Partial<Record<keyof typeof SECTIONS, ComponentType>> = {
  cli: CliSettingsActions,
};

export function SettingsView() {
  const { t } = useTranslation();
  const section = useUi((s) => s.settingsSection);
  const Section = SECTIONS[section];
  const Actions = ACTIONS[section];

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="sticky top-0 z-10 border-b border-border bg-bg/95 backdrop-blur">
          <div className="mx-auto flex max-w-2xl items-center justify-between gap-4 px-4 py-4 sm:px-8">
            <h1 className="text-lg font-semibold">{t(`settings.sections.${section}`)}</h1>
            {Actions && <Actions />}
          </div>
        </div>
        <div className="mx-auto max-w-2xl px-4 pt-6 pb-44 sm:px-8">
          <Section />
      </div>
    </div>
  );
}
