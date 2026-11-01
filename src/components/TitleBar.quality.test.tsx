// @vitest-environment happy-dom
import { beforeEach, afterEach, it, expect, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("./ProviderStatusMenu", () => ({ ProviderStatusMenu: () => null }));
vi.mock("../lib/layout", () => ({ useLayout: () => ({ docked: 0 }) }));

import { TitleBar } from "./TitleBar";
import { useUi } from "../stores/ui";

let host: HTMLDivElement, root: Root;

beforeEach(() => {
  (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

it("renders streamlined title bar with single dashboard button and no redundant pill text", async () => {
  await act(async () => {
    root.render(createElement(TitleBar));
  });

  // Verify no redundant quality or savings pill text
  expect(host.textContent).not.toContain("no verified observations");
  expect(host.textContent).not.toContain("首次通过");
  expect(host.textContent).not.toContain("First-pass");
  expect(host.textContent).not.toContain("100%");

  // Verify single dashboard action icon button is present
  const buttons = Array.from(host.querySelectorAll("button"));
  const dashboardBtn = buttons.find((btn) => btn.getAttribute("aria-label")?.toLowerCase().includes("dashboard"));
  expect(dashboardBtn).toBeDefined();
});

it("triggers openDashboard when dashboard button is clicked", async () => {
  const openDashboard = vi.fn();
  useUi.setState({ openDashboard });

  await act(async () => {
    root.render(createElement(TitleBar));
  });

  const buttons = Array.from(host.querySelectorAll("button"));
  const dashboardBtn = buttons.find((btn) => btn.getAttribute("aria-label")?.toLowerCase().includes("dashboard"));
  expect(dashboardBtn).toBeDefined();

  act(() => {
    dashboardBtn!.click();
  });

  expect(openDashboard).toHaveBeenCalled();
});
