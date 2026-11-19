// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it } from "vitest";
import "../i18n";
import type { ChatMessage, ToolPart } from "../stores/chat";
import { AssistantReply } from "./Conversation";

const code = 'const newline = "\\n";\nconsole.log(newline);';
const structured = JSON.stringify({ stdout: code, exitCode: 0 });
const mixed = JSON.stringify({ items: [{ Text: code }, { status: "ok" }] });
const truncated = '{"items":[{"Text":"a\\n';

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
});

it.each([
  { kind: "Kiro text blocks", provider: "KIRO_CLI", output: JSON.stringify({ items: [{ Text: code }, { Text: "Done" }] }), expected: code + "\nDone" },
  { kind: "a Kiro text string", provider: "KIRO_CLI", output: JSON.stringify(code), expected: code },
  { kind: "Kiro text containing JSON", provider: "KIRO_CLI", output: JSON.stringify(structured), expected: structured },
  { kind: "structured Kiro data", provider: "KIRO_CLI", output: structured, expected: structured },
  { kind: "mixed Kiro data", provider: "KIRO_CLI", output: mixed, expected: mixed },
  { kind: "truncated Kiro data", provider: "KIRO_CLI", output: truncated, expected: truncated },
  { kind: "Codex output resembling a JSON string", provider: "CODEX_CLI", output: JSON.stringify(code), expected: JSON.stringify(code) },
] as const)("renders $kind without changing literal escapes", async ({ provider, output, expected }) => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const detail = "First line\nSecond line with literal \\n";
  const tool: ToolPart = { type: "tool", id: "read", tool: "read", title: "example.js", detail, ok: true, done: true, output, files: [] };
  const message: ChatMessage = { id: "message", role: "assistant", text: "", status: "done", provider, parts: [tool] };

  try {
    await act(async () => root.render(<AssistantReply m={message} sessionId="session" />));
    await act(async () => host.querySelector<HTMLButtonElement>("button[aria-expanded]")!.click());
    await act(async () => [...host.querySelectorAll("button")].find(button => button.textContent?.includes(tool.title))!.click());

    expect(host.querySelector("pre")?.textContent).toBe(expected);
    const description = [...host.querySelectorAll("p")].find(p => p.textContent === detail);
    expect(description?.classList.contains("whitespace-pre-wrap")).toBe(true);
  } finally {
    await act(async () => root.unmount());
  }
});
