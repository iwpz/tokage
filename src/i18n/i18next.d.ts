import "i18next";
import type { Resources } from "./locales/en";

declare module "i18next" {
  interface CustomTypeOptions {
    resources: { translation: Resources };
  }
}
