// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
const mocks = vi.hoisted(() => ({ state: {} as any, command: vi.fn(), refresh: vi.fn(), invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", async original => ({ ...await original<object>(), invoke: mocks.invoke }));
vi.mock("../../stores/runtime", () => ({
 useRuntime: Object.assign((select: (state: unknown) => unknown) => select(mocks.state), { getState: () => mocks.state }),
 observeRuntime: vi.fn(() => () => {}),
}));
import i18n from "../../i18n";
import { RuntimeView } from "./RuntimeView";
import { useProjects } from "../../stores/projects";
import { runtimeUiSnapshot, runtimeUiTranscripts } from "../../../tests/fixtures/runtime-ui";
let host: HTMLDivElement; let root: Root;
beforeEach(async () => {
 vi.useFakeTimers(); vi.clearAllMocks();
 (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
 Element.prototype.scrollIntoView = vi.fn();
 window.confirm = vi.fn(() => true);
 await i18n.changeLanguage("en");
 mocks.state = { runs: { run: { snapshot: structuredClone(runtimeUiSnapshot), transcripts: structuredClone(runtimeUiTranscripts) } }, errors: {}, command: mocks.command, refresh: mocks.refresh };
 mocks.refresh.mockResolvedValue(undefined); mocks.command.mockResolvedValue(undefined); mocks.invoke.mockResolvedValue("# Rendered edited plan");
 useProjects.setState({ projects: [{ id: "project", path: "/project", name: "Project", sessions: [{ id: "run", title: "Run", state: "review" }], trusted: true }] as any });
 host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); vi.restoreAllMocks(); });
async function render(view: "chat" | "plan" | "pipeline" | "session") { await act(async () => { root.render(<RuntimeView sessionId="run" view={view} />); for (let i = 0; i < 10; i++) await Promise.resolve(); }); }
async function click(button: Element | undefined | null) { expect(button).toBeTruthy(); await act(async () => { (button as HTMLElement).click(); }); }
const button = (label: string) => [...host.querySelectorAll("button")].find(b => b.textContent?.trim() === label);
it("switches the real Spec, Contracts and Tasks editors and preserves runtime ownership", async () => {
 await render("plan"); expect(host.textContent).toContain("Structured feature goal");
 const tabs = () => [...host.querySelectorAll('[role="tab"]')];
 await click(tabs()[1]); expect(host.textContent).toContain("Public contract"); expect(host.querySelector("strong")?.textContent).toBe("API contract");
 await click(tabs()[2]); expect(host.textContent).toContain("Implement feature"); expect(host.textContent).toContain("Claude · Haiku");
 expect(mocks.command).not.toHaveBeenCalled(); expect(mocks.invoke).not.toHaveBeenCalled();
});
it("keeps approval disabled until exact commands are confirmed, then submits only the explicit runtime intent", async () => {
 await render("pipeline");
 await click(button(i18n.t("runtime.openApproval")));
 const approve = button(i18n.t("runtime.approve")) as HTMLButtonElement;
 expect(approve.disabled).toBe(true); await click([...host.querySelectorAll('label')].find(label => label.textContent === 'npm test')?.querySelector('input')); expect(approve.disabled).toBe(false);
 await click(approve);
 expect(mocks.command).toHaveBeenCalledExactlyOnceWith("run", { type: "ConfirmPlan", payload: { confirmationDigest: "digest", confirmedCommands: ["npm test"] } });
});
it("switches pipeline card/table views and opens task acceptance details", async () => {
 await render("pipeline"); await click(host.querySelector(`[aria-label="${i18n.t("runtime.table")}"]`));
 expect(host.querySelector("table")).not.toBeNull(); await click(host.querySelector("tbody tr"));
 const details = host.querySelector("aside"); expect(details?.textContent).toContain("Feature works"); expect(details?.textContent).toContain("npm test");
 expect(mocks.command).not.toHaveBeenCalled();
});
it("reconstructs the model tree and selected messages in Session from durable runtime projections", async () => {
 await render("session"); await act(async () => { await vi.advanceTimersByTimeAsync(0); });
 expect(host.textContent).toContain("claude-haiku-5-5"); expect(host.textContent).toContain("User requirement"); expect(host.textContent).toContain("Hello");
 expect(host.querySelector('[data-message-role="ASSISTANT"]')).not.toBeNull(); expect(mocks.command).not.toHaveBeenCalled();
});
it("saves structured edits through runtime validation and never writes the legacy plan store", async () => {
 await render("plan"); await click(button(i18n.t("plan.spec.edit")));
 const textarea = host.querySelector("textarea")!;
 await act(async () => { const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!; setter.call(textarea, "Edited feature goal"); textarea.dispatchEvent(new Event("input", { bubbles: true })); textarea.dispatchEvent(new Event("change", { bubbles: true })); });
 const save = button(i18n.t("plan.review.save")) as HTMLButtonElement;
 expect(save.disabled).toBe(false); await click(save);
 expect(mocks.invoke).toHaveBeenCalledWith("runtime_render_plan", { plan: expect.objectContaining({ spec: expect.objectContaining({ goal: "Edited feature goal" }) }) });
 expect(mocks.command).toHaveBeenCalledExactlyOnceWith("run", { type: "ImportPlan", payload: { text: "# Rendered edited plan" } });
});
it("routes explicit task retry through runtime and leaves task state authoritative", async () => {
 const s = mocks.state.runs.run.snapshot;
 s.state = "Blocked";
 s.execution = { sequence: 1, tasks: { T1: { state: "BLOCKED", assignment: s.plan.assignments.T1, attempts: [], error: "command failed", deviation: false, repairStep: "Initial" } } };
 await render("pipeline"); await click(button(i18n.t("runtime.retryTask")));
 expect(mocks.command).toHaveBeenCalledExactlyOnceWith("run", { type: "RetryTask", payload: { taskId: "T1" } });
 expect(s.execution.tasks.T1.state).toBe("BLOCKED");
});
it("keeps an edited Markdown Spec and the canonical contract/task sections when saving", async () => {
 mocks.invoke.mockResolvedValue("# Spec\n\n## Goals\nOld\n\n# Contracts\nC1\n\n# Tasks\n```yaml\n- id: T1\n```\n");
 await render("plan"); await click(button(i18n.t("plan.spec.markdown")));
 const textarea = host.querySelector("textarea")!;
 await act(async () => { Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "## Goals\nNew Markdown goal"); textarea.dispatchEvent(new Event("input", { bubbles: true })); });
 await click(button(i18n.t("plan.review.save")));
 expect(mocks.command).toHaveBeenCalledExactlyOnceWith("run", { type: "ImportPlan", payload: { text: "# Spec\n\n## Goals\nNew Markdown goal\n\n# Contracts\nC1\n\n# Tasks\n```yaml\n- id: T1\n```\n" } });
});

it("replans a failed initial planner turn without offering execution retry with a missing digest", async () => {
 const s=mocks.state.runs.run.snapshot;s.state="Blocked";delete s.plan;s.planningAttempt={error:"OAuth token revoked"};
 await render("pipeline");
 expect(button(i18n.t("runtime.retry"))).toBeUndefined();
 await click(button(i18n.t("runtime.generate")));
 expect(mocks.command).toHaveBeenCalledExactlyOnceWith("run",{type:"GeneratePlan",payload:undefined});
});
it("retains explicit execution retry against the available plan digest", async () => {
 mocks.state.runs.run.snapshot.state="Blocked";
 await render("pipeline");await click(button(i18n.t("runtime.retry")));
 expect(mocks.command).toHaveBeenCalledExactlyOnceWith("run",{type:"PrepareRetry",payload:{confirmationDigest:"digest"}});
});
