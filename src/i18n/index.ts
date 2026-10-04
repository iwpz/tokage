import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en";
import zh from "./locales/zh";
import { useSettings } from "../stores/settings";

export const LANGUAGES = [
  { code: "en", label: "English" },
  { code: "zh", label: "简体中文" },
] as const;

export type Language = (typeof LANGUAGES)[number]["code"];

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, zh: { translation: zh } },
  lng: useSettings.getState().language,
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

const syncHtmlLang = (lng: string) => {
  document.documentElement.lang = lng === "zh" ? "zh-CN" : "en";
};
syncHtmlLang(i18n.language);

// Keep i18next in sync with the persisted setting.
useSettings.subscribe((state, prev) => {
  if (state.language !== prev.language) {
    void i18n.changeLanguage(state.language);
    syncHtmlLang(state.language);
  }
});

export default i18n;
