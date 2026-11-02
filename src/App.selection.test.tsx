// @vitest-environment happy-dom
import { beforeEach, afterEach, describe, it, expect } from "vitest";

describe("Native desktop app text selection constraints", () => {
  let container: HTMLDivElement;
  let listeners: Array<{ type: string; listener: (e: any) => void }> = [];

  const addTrackedListener = (type: string, listener: (e: any) => void) => {
    window.addEventListener(type, listener);
    listeners.push({ type, listener });
  };

  beforeEach(() => {
    container = document.createElement("div");
    document.body.append(container);

    // Setup the App-level contextmenu, selectstart, and keydown listeners
    const onContextMenu = (e: MouseEvent) => {
      const target = e.target as HTMLElement | null;
      const isInput = !!target?.closest("input, textarea, [contenteditable='true']");
      const inSelectable = !!target?.closest("[data-selectable]");
      const hasSelection = !window.getSelection()?.isCollapsed && !!window.getSelection()?.toString().trim();
      if (!isInput && !(inSelectable && hasSelection)) {
        e.preventDefault();
      }
    };
    addTrackedListener("contextmenu", onContextMenu);

    const onSelectStart = (e: Event) => {
      const target = e.target as HTMLElement | null;
      const isSelectable = !!target?.closest("input, textarea, [contenteditable='true'], [data-selectable]");
      if (!isSelectable) {
        e.preventDefault();
      }
    };
    addTrackedListener("selectstart", onSelectStart);

    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === "a") {
        const activeEl = document.activeElement as HTMLElement | null;
        const isInput = !!activeEl?.closest("input, textarea, [contenteditable='true']");
        if (!isInput) {
          const selection = window.getSelection();
          const anchorNode = selection?.anchorNode;
          const selectableContainer =
            anchorNode instanceof HTMLElement
              ? anchorNode.closest("[data-selectable]")
              : anchorNode?.parentElement?.closest("[data-selectable]");

          if (selectableContainer && selection) {
            e.preventDefault();
            const range = document.createRange();
            range.selectNodeContents(selectableContainer);
            selection.removeAllRanges();
            selection.addRange(range);
          } else {
            e.preventDefault();
          }
        }
      }
    };
    addTrackedListener("keydown", onKey);
  });

  afterEach(() => {
    for (const { type, listener } of listeners) {
      window.removeEventListener(type, listener);
    }
    listeners = [];
    container.remove();
  });

  it("blocks drag-to-select on standard UI text but permits on input and data-selectable content", () => {
    const regularUiLabel = document.createElement("span");
    regularUiLabel.textContent = "Tokage App Title";
    container.append(regularUiLabel);

    const regularEvent = new Event("selectstart", { bubbles: true, cancelable: true });
    regularUiLabel.dispatchEvent(regularEvent);
    expect(regularEvent.defaultPrevented).toBe(true);

    const textInput = document.createElement("input");
    container.append(textInput);
    const inputEvent = new Event("selectstart", { bubbles: true, cancelable: true });
    textInput.dispatchEvent(inputEvent);
    expect(inputEvent.defaultPrevented).toBe(false);

    const selectablePath = document.createElement("span");
    selectablePath.setAttribute("data-selectable", "");
    selectablePath.textContent = "/path/to/project";
    container.append(selectablePath);
    const pathEvent = new Event("selectstart", { bubbles: true, cancelable: true });
    selectablePath.dispatchEvent(pathEvent);
    expect(pathEvent.defaultPrevented).toBe(false);
  });

  it("suppresses Cmd+A global page selection when focused outside input or selectable block", () => {
    const generalButton = document.createElement("button");
    generalButton.textContent = "Settings Tab";
    container.append(generalButton);
    generalButton.focus();

    const cmdA = new KeyboardEvent("keydown", {
      key: "a",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(cmdA);
    expect(cmdA.defaultPrevented).toBe(true);
  });

  it("preserves Cmd+A selection when focused inside input element", () => {
    const textarea = document.createElement("textarea");
    container.append(textarea);
    textarea.focus();

    const cmdA = new KeyboardEvent("keydown", {
      key: "a",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(cmdA);
    expect(cmdA.defaultPrevented).toBe(false);
  });

  it("scopes Cmd+A to the specific data-selectable container when active in it", () => {
    const aiOutput = document.createElement("div");
    aiOutput.setAttribute("data-selectable", "");
    aiOutput.textContent = "AI generated answer and analysis.";
    container.append(aiOutput);

    const range = document.createRange();
    range.selectNodeContents(aiOutput);
    const sel = window.getSelection();
    sel?.removeAllRanges();
    sel?.addRange(range);

    const cmdA = new KeyboardEvent("keydown", {
      key: "a",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(cmdA);
    // Prevents global page selection
    expect(cmdA.defaultPrevented).toBe(true);
    // But keeps the range scoped to the container
    expect(sel?.toString()).toBe("AI generated answer and analysis.");
  });
});
