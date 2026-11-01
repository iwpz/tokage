// @vitest-environment happy-dom
import { beforeEach, afterEach, it, expect, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

const api = vi.hoisted(() => ({
  pickDir: vi.fn(),
}));

vi.mock("../lib/ipc", async (original) => ({
  ...await original<object>(),
  pickProjectDirectory: api.pickDir,
}));

vi.mock("../lib/layout", () => ({
  useLayout: () => ({ win: { w: 1200, h: 800 }, compact: false, sidebarWidth: 260 }),
  sidebarMax: () => 400,
}));

import { Sidebar } from "./Sidebar";
import { useProjects } from "../stores/projects";
import { useUi } from "../stores/ui";

let host: HTMLDivElement, root: Root;

beforeEach(() => {
  (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
  api.pickDir.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);

  useProjects.setState({
    projects: [
      {
        id: "proj-1",
        name: "TestProject",
        path: "/path/to/test-project",
        expanded: true,
        sessions: [],
      },
    ],
    activeProjectId: "proj-1",
    activeSessionId: null,
  });

  useUi.setState({
    sidebarCollapsed: false,
    view: "workspace",
  });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

it("invokes pickProjectDirectory and adds a project when clicking FolderPlus button", async () => {
  api.pickDir.mockResolvedValue("/path/to/new-project");

  await act(async () => {
    root.render(createElement(Sidebar));
  });

  const buttons = Array.from(host.querySelectorAll("button"));
  const addProjectBtn = buttons.find((btn) => btn.getAttribute("aria-label")?.includes("project") || btn.getAttribute("aria-label")?.includes("项目"));
  expect(addProjectBtn).toBeDefined();

  await act(async () => {
    addProjectBtn!.click();
  });

  expect(api.pickDir).toHaveBeenCalled();
  const projects = useProjects.getState().projects;
  expect(projects.some((p) => p.path === "/path/to/new-project")).toBe(true);
});

it("creates a new session under the project when clicking the Plus button on the project row", async () => {
  await act(async () => {
    root.render(createElement(Sidebar));
  });

  const buttons = Array.from(host.querySelectorAll("button"));
  const newSessionBtn = buttons.find((btn) =>
    btn.getAttribute("aria-label")?.includes("session") ||
    btn.getAttribute("aria-label")?.includes("Session") ||
    btn.getAttribute("aria-label")?.includes("会话")
  );
  expect(newSessionBtn).toBeDefined();

  await act(async () => {
    newSessionBtn!.click();
  });

  const state = useProjects.getState();
  expect(state.projects[0].sessions.length).toBe(1);
  expect(state.activeSessionId).toBe(state.projects[0].sessions[0].id);
});
