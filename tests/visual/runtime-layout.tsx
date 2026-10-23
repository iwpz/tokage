// Isolated browser rendering of components, not an application acceptance driver.
import { createRoot } from "react-dom/client";
import "../../src/index.css";
import i18n from "../../src/i18n";
import { RuntimeView } from "../../src/components/planning/RuntimeView";
import { useProjects } from "../../src/stores/projects";
import { snapshot } from "./runtime.fixture";
import { useRuntime } from "../../src/stores/runtime";
await i18n.changeLanguage("en");
useProjects.setState({ projects: [{ id: "fixture", path: "/project", name: "Offline fixture", trusted: true, sessions: [{ id: "run", title: "Offline layout fixture", state: "review" }] }] as never });
await useRuntime.getState().refresh(snapshot.runId);
const view = new URLSearchParams(location.search).get("view") === "pipeline" ? "pipeline" : "plan";
createRoot(document.getElementById("root")!).render(<div className="flex h-screen min-w-0 flex-col overflow-hidden bg-base text-fg"><header className="h-14 shrink-0 border-b border-border px-5 py-4 text-xs text-muted">Offline component layout fixture — no application data or model calls</header><nav className="h-12 shrink-0 border-b border-border px-5 py-3 text-sm">Chat · Plan · Pipeline · Session</nav><main data-testid="workspace-content" className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"><RuntimeView sessionId="run" view={view}/></main></div>);
