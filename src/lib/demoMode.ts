import i18n from "../i18n";
import { create } from "zustand";
import { useProjects } from "../stores/projects";
import { useChat } from "../stores/chat";
import { useExecution } from "../stores/execution";
import { useSettings } from "../stores/settings";
import { useUi } from "../stores/ui";
import { auditDemoEvent, auditDemoEnded } from "./demoAudit";
import { onChatEvent, onChatEnded, parsePlanningOutput, persistPlan, prepareDebugWorkspace, seedDemoBrowserFiles, validatePlan } from "./ipc";
import { DEMO_PROJECT_ID, demoConfig, demoIdentity } from "./demoIdentity";
import { demoScript } from "./demoScript";
import { plannedDebugRoute } from "./debugProvider";

export const useDemoStatus = create<{ busy: boolean; error: string | null }>(() => ({ busy: false, error: null }));
let pending: Promise<void> | null = null;
let auditReady: Promise<unknown> | undefined;
export function ensureDemoAudit() {
  return auditReady ??= Promise.all([onChatEvent(auditDemoEvent), onChatEnded(auditDemoEnded)]).catch(error => { auditReady = undefined; throw error; });
}

/** A new run gets a fresh repository and identities; old runs can still be inspected on disk. */
export function prepareDemo(reset = false): Promise<void> {
  if (pending) return pending;
  pending = initialize(reset).finally(() => { pending = null; useDemoStatus.setState({ busy: false }); });
  return pending;
}

async function initialize(reset: boolean) {
  useDemoStatus.setState({ busy: true, error: null });
  try {
    await ensureDemoAudit();
    const existing = useProjects.getState().projects.find(p => p.id === DEMO_PROJECT_ID);
    if (existing && !reset) return;
    if (existing?.sessions.some(s => {
      const pipeline = useExecution.getState().pipelines[s.id];
      return Object.values(pipeline?.taskStates ?? {}).some(state => ["RUNNING", "VERIFYING", "REPAIRING"].includes(state)) || useChat.getState().conversations[s.id]?.messages.some(m => m.status === "streaming");
    })) throw new Error(i18n.t("demo.activeReset"));
    const settings = useSettings.getState();
    const language = settings.language;
    const scenario = settings.demoScenario;
    const run = crypto.randomUUID();
    const sessionId = `tokage-demo-${scenario}-${language}-${run}`;
    const cwd = await prepareDebugWorkspace(`tokage-demo:${run}`);
    seedDemoBrowserFiles(cwd);
    const script = demoScript(language);
    const plan = await parsePlanningOutput({ markdown: script.plan.rawMarkdown });
    const validation = await validatePlan({ plan, allowedCommands: ["npm test", "npm run build"] });
    if (!validation.isValid) throw new Error(validation.issues.map(i => i.message).join("\n"));
    await persistPlan(cwd, sessionId, validation.plan);
    const config = demoConfig(sessionId, settings.demoChunkDelayMs)!;
    const text = `${script.intro}\n\n${script.plan.rawMarkdown}`;
    useChat.setState(s => ({ conversations: { ...s.conversations, [sessionId]: {
      thread: null,
      messages: [
        { id: `${sessionId}-request`, role: "user", text: script.request },
        { id: `${sessionId}-plan`, role: "assistant", text, mode: "plan", modelName: "Fable 5.1", provider: "CLAUDE_CODE", model: "fable", cwd, status: "done", parts: [{ type: "text", text }], debugRoute: plannedDebugRoute("CLAUDE_CODE", "fable", cwd, config), usage: { input: 0, output: 0, apiCalls: 0, costUsd: 0, source: "STRUCTURED" } },
      ],
    } } }));
    useProjects.setState(s => ({
      projects: [...s.projects.filter(p => p.id !== DEMO_PROJECT_ID), {
        id: DEMO_PROJECT_ID, name: "demo_project", path: cwd, expanded: true, trusted: true,
        sessions: [{ id: sessionId, title: "demo_work", state: "review", updatedAt: Date.now() }],
      }], activeProjectId: DEMO_PROJECT_ID, activeSessionId: sessionId,
    }));
    // Drop replaced in-memory runs only after the new fixture has been prepared successfully.
    for (const session of existing?.sessions ?? []) {
      useExecution.getState().discardPipeline(session.id);
      useChat.setState(s => ({ conversations: Object.fromEntries(Object.entries(s.conversations).filter(([id]) => id !== session.id)) }));
    }
    useSettings.getState().set("demoEnabled", true);
    useUi.getState().setWorkspaceTab("chat");
  } catch (error) {
    useDemoStatus.setState({ error: String(error) });
    throw error;
  }
}

export function showDemo() {
  const project = useProjects.getState().projects.find(p => p.id === DEMO_PROJECT_ID);
  const session = project?.sessions.find(s => demoIdentity(s.id));
  if (project && session) useProjects.getState().selectSession(project.id, session.id);
  useUi.getState().closeSettings();
  useUi.getState().setWorkspaceTab("chat");
}

export function hideDemo() {
  useSettings.getState().set("demoEnabled", false);
  const state = useProjects.getState();
  if (state.activeProjectId === DEMO_PROJECT_ID) {
    const project = state.projects.find(p => p.id !== DEMO_PROJECT_ID);
    useProjects.setState({ activeProjectId: project?.id ?? null, activeSessionId: null });
  }
}
