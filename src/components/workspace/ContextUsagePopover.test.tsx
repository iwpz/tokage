// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { taskContextInputs } from "../../../tests/fixtures/runtime-context";
import i18n from "../../i18n";
import { ContextUsagePopover, ContextUsageRow } from "./ContextUsagePopover";

it("opens on click, preserves measured zero, and reports unknown usage without a progress bar", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const inputs = {
    ...taskContextInputs,
    params: { ...taskContextInputs.params, effectiveContext: 128_000 },
    sources: { totalTokens: "CLI_CONTEXT_TOKENS" },
    totalTokens: 0,
    warmNow: true,
    cacheTtlRemainingSeconds: 240,
  };
  try {
    await i18n.changeLanguage("en");
    await act(async () => root.render(
      <ContextUsagePopover inputs={inputs}>
        <ContextUsageRow label="Main agent" inputs={inputs} />
        <ContextUsageRow label="T2" inputs={null} />
      </ContextUsagePopover>,
    ));
    const button = host.querySelector("button")!;
    const panel = document.getElementById(button.getAttribute("aria-controls")!)!;
    expect(panel.hidden).toBe(true);
    expect(host.querySelector("progress")).toBeNull();

    await act(async () => button.click());
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(panel.hidden).toBe(false);
    expect(panel.textContent).toContain("Context usage unavailable");
    expect(panel.textContent).not.toMatch(/cache|04:00/);
    expect(panel.querySelectorAll("progress")).toHaveLength(1);
    expect(panel.querySelector("progress")?.getAttribute("value")).toBe("0");

    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(panel.hidden).toBe(true);
    await act(async () => button.click());
    await act(async () => document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })));
    expect(button.getAttribute("aria-expanded")).toBe("false");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});
