import { useEffect } from "react";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { TrustPrompt } from "./components/TrustPrompt";
import { Workspace } from "./components/Workspace";
import { PreviewPane } from "./components/PreviewPane";
import { SettingsView } from "./components/settings/SettingsView";
import { useApplyTheme } from "./lib/theme";
import { useProviders } from "./stores/providers";
import { useUi } from "./stores/ui";

export default function App() {
  useApplyTheme();
  const view = useUi((s) => s.view);
  const loadProviders = useProviders((s) => s.load);

  // Cached CLI status renders immediately (persisted store); a fresh scan updates it in place.
  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  // ⌘, opens settings, Esc leaves them, ⌘B toggles the sidebar.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const ui = useUi.getState();
      if (mod && e.key === ",") {
        e.preventDefault();
        ui.openSettings();
      } else if (mod && e.key.toLowerCase() === "b") {
        e.preventDefault();
        ui.toggleSidebar();
      } else if (e.key === "Escape" && ui.view === "settings") {
        ui.closeSettings();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <TitleBar />
      <div className="relative flex min-h-0 flex-1">
        <Sidebar />
        <main className="flex min-w-0 flex-1 flex-col bg-bg">
          {view === "settings" ? <SettingsView /> : <Workspace />}
        </main>
        {view === "workspace" && <PreviewPane />}
      </div>
      <TrustPrompt />
    </div>
  );
}
