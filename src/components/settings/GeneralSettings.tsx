import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Monitor, Moon, RefreshCw, Sun } from "lucide-react";
import { LANGUAGES } from "../../i18n";
import { reloadShellEnv } from "../../lib/ipc";
import { useSettings, type ThemePreference } from "../../stores/settings";
import { Button, Card, Segmented, Select, SettingRow } from "../ui";

export function GeneralSettings() {
  const { t } = useTranslation();
  const { theme, language, set } = useSettings();
  const [reloading, setReloading] = useState(false);

  return (
    <Card>
      <SettingRow title={t("settings.general.appearance")} description={t("settings.general.appearanceDesc")}>
        <Segmented<ThemePreference>
          label={t("settings.general.appearance")}
          value={theme}
          onChange={(v) => set("theme", v)}
          options={[
            { value: "system", label: t("settings.general.theme.system"), icon: <Monitor className="size-3.5" /> },
            { value: "light", label: t("settings.general.theme.light"), icon: <Sun className="size-3.5" /> },
            { value: "dark", label: t("settings.general.theme.dark"), icon: <Moon className="size-3.5" /> },
          ]}
        />
      </SettingRow>
      <SettingRow title={t("settings.general.language")} description={t("settings.general.languageDesc")}>
        <Select
          label={t("settings.general.language")}
          value={language}
          onChange={(v) => set("language", v)}
          options={LANGUAGES.map((l) => ({ value: l.code, label: l.label }))}
        />
      </SettingRow>
      <SettingRow title={t("settings.general.reloadEnv")} description={t("settings.general.reloadEnvDesc")}>
        <Button
          disabled={reloading}
          onClick={async () => {
            setReloading(true);
            try {
              await reloadShellEnv();
            } finally {
              setReloading(false);
            }
          }}
        >
          <RefreshCw className={reloading ? "size-3.5 animate-spin" : "size-3.5"} />
          {t("settings.general.reload")}
        </Button>
      </SettingRow>
    </Card>
  );
}
