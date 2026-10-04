import { useEffect } from "react";
import { useSettings } from "../stores/settings";

const media = window.matchMedia("(prefers-color-scheme: dark)");

/** Applies the `.dark` class on <html> based on the theme preference. */
export function useApplyTheme() {
  const theme = useSettings((s) => s.theme);

  useEffect(() => {
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && media.matches);
      document.documentElement.classList.toggle("dark", dark);
    };
    apply();
    if (theme !== "system") return;
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
}
