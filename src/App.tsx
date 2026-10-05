import { useEffect } from "react";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { TrustPrompt } from "./components/TrustPrompt";
import { WelcomeNotice } from "./components/WelcomeNotice";
import { Workspace } from "./components/Workspace";
import { PreviewPane } from "./components/PreviewPane";
import { SettingsView } from "./components/settings/SettingsView";
import { UsageDashboardView } from "./components/dashboard/UsageDashboardView";
import { StorageBanner } from "./components/StorageBanner";
import { useApplyTheme } from "./lib/theme";
import { useProviders } from "./stores/providers";
import { useSettings } from "./stores/settings";
import { useUi } from "./stores/ui";

export default function App() {
  useApplyTheme();
  const view = useUi((s) => s.view);
  const loadProviders = useProviders((s) => s.load);
  const welcomeSeen = useSettings((s) => s.welcomeSeen);

  // Cached CLI status renders immediately (restored from the database); a fresh scan updates it in place.
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
      } else if (e.key === "Escape") {
        if (ui.view === "settings") {
          ui.closeSettings();
        } else if (ui.view === "dashboard") {
          ui.closeDashboard();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <TitleBar />
      <StorageBanner />
      <div className="relative flex min-h-0 flex-1">
        <Sidebar />
        <main className="flex min-w-0 flex-1 flex-col bg-bg">
          {view === "settings" ? (
            <SettingsView />
          ) : view === "dashboard" ? (
            <UsageDashboardView />
          ) : (
            <Workspace />
          )}
        </main>
        {view === "workspace" && <PreviewPane />}
      </div>
      {/* One modal at a time: workspace trust is asked after the first-launch notice. */}
      {welcomeSeen ? <TrustPrompt /> : <WelcomeNotice />}
    </div>
  );
}
