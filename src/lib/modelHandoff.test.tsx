// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";

const hoisted = vi.hoisted(() => ({
  startChatTurnMock: vi.fn(),
  cancelChatTurnMock: vi.fn(),
  chatEventHandler: null as ((e: any) => void) | null,
}));

vi.mock("./ipc", async () => {
  const actual = await vi.importActual<typeof import("./ipc")>("./ipc");
  return {
    ...actual,
    inTauri: false,
    startChatTurn: (...args: unknown[]) => hoisted.startChatTurnMock(...args),
    cancelChatTurn: (...args: unknown[]) => hoisted.cancelChatTurnMock(...args),
    onChatEvent: (fn: (e: any) => void) => {
      hoisted.chatEventHandler = fn;
      return Promise.resolve(() => {});
    },
  };
});

import { useChat, type ChatMessage } from "../stores/chat";
import { useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";
import { Conversation } from "../components/Conversation";
import {
  collectSessionHandoffSnapshot,
  extractApiAndContractSnippets,
  getActiveSessionModelRef,
  getReadySmartHandoffMessage,
  hasUnsentSmartHandoff,
} from "./modelHandoff";

describe("single-model switch & smart handoff", () => {
  const sessionId = "sess-handoff-1";
  const modelA = { provider: "CLAUDE_CODE" as const, model: "sonnet" };
  const modelB = { provider: "CODEX_CLI" as const, model: "gpt-5" };
  const modelC = { provider: "CURSOR_CLI" as const, model: "gemini-pro" };
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
    hoisted.startChatTurnMock.mockReset();
    hoisted.cancelChatTurnMock.mockReset();
    hoisted.startChatTurnMock.mockResolvedValue(undefined);
    hoisted.cancelChatTurnMock.mockResolvedValue(undefined);

    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);

    useProjects.setState({
      projects: [
        {
          id: "proj-1",
          name: "tokage",
          path: "/tmp/tokage",
          expanded: true,
          trusted: true,
          sessions: [{ id: sessionId, title: "Handoff Test", state: "draft", updatedAt: Date.now() }],
        },
      ],
      activeProjectId: "proj-1",
      activeSessionId: sessionId,
    });

    useSettings.setState({
      contextMode: "off",
      providerEnabled: { CLAUDE_CODE: true, CODEX_CLI: true, CURSOR_CLI: true },
      providerTrust: { CLAUDE_CODE: true, CODEX_CLI: true, CURSOR_CLI: true },
      blockedModels: [],
      modelBanningLevel: "off",
      modelProviderPreferences: {},
      modelChannelAccess: {},
    });

    useProviders.setState({ providers: [] });

    const initialMessages: ChatMessage[] = [
      {
        id: "u1",
        role: "user",
        text: "Design and implement the HandoffService API",
      },
      {
        id: "a1",
        role: "assistant",
        text: "I have implemented the core contract:\n```ts\nexport interface HandoffContract {\n  sessionId: string;\n  version: number;\n}\nexport function executeHandoff(req: HandoffContract): Promise<boolean>\n```",
        mode: "direct",
        provider: "CLAUDE_CODE",
        ref: modelA,
        model: "sonnet",
        modelName: "Claude Sonnet",
        variant: { id: "sonnet", effort: null, fast: false, tier: null },
        status: "done",
        cwd: "/tmp/tokage",
        files: [
          {
            path: "src/handoff.ts",
            absPath: "/tmp/tokage/src/handoff.ts",
            change: "added",
            diff: "+++ b/src/handoff.ts\n+export interface HandoffContract { sessionId: string; }",
            additions: 12,
            deletions: 0,
          },
        ],
        parts: [
          {
            type: "tool",
            id: "t1",
            tool: "edit",
            title: "src/handoff.ts",
            detail: null,
            ok: true,
            done: true,
            output: null,
            files: ["src/handoff.ts"],
          },
          {
            type: "text",
            text: "Implemented `src/handoff.ts` with `HandoffContract`.",
          },
        ],
      },
    ];

    useChat.setState({
      conversations: {
        [sessionId]: {
          messages: initialMessages,
          thread: { ref: modelA, id: "thread-claude-1", mode: "direct" },
          lockedModel: modelA,
        },
      },
      picks: { plan: null, direct: null, fixed: modelA },
      modelOptions: {},
    });
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });

  const renderConversation = async () => {
    await act(async () => {
      root.render(<Conversation sessionId={sessionId} />);
    });
  };

  it("extracts active session model, APIs, contracts, and file changes comprehensively", async () => {
    const conv = useChat.getState().conversations[sessionId];
    const active = getActiveSessionModelRef(conv);
    expect(active).toEqual({ ref: modelA, modelName: "Claude Sonnet" });

    const apis = extractApiAndContractSnippets(conv.messages, [conv.messages[1].files?.[0]?.diff]);
    expect(apis.some((s) => s.includes("export interface HandoffContract"))).toBe(true);
    expect(apis.some((s) => s.includes("export function executeHandoff"))).toBe(true);

    const snapshot = await collectSessionHandoffSnapshot({
      conv,
      sessionId,
      cwd: "/tmp/tokage",
      fromRef: modelA,
      fromModelName: "Claude Sonnet",
      toRef: modelB,
      toModelName: "GPT-5",
    });
    expect(snapshot.initialGoal).toContain("Design and implement the HandoffService API");
    expect(snapshot.fileChanges).toHaveLength(1);
    expect(snapshot.fileChanges[0].path).toBe("src/handoff.ts");
    expect(snapshot.extractedApisAndContracts.length).toBeGreaterThan(0);
  });

  it("records direct model switch, renders switch notice in UI, and sends full history on next turn", async () => {
    useChat.getState().recordDirectModelSwitch(sessionId, {
      fromRef: modelA,
      fromModelName: "Claude Sonnet",
      toRef: modelB,
      toModelName: "GPT-5",
    });

    await renderConversation();
    const notice = host.querySelector('[data-testid="model-switch-notice"]');
    expect(notice?.textContent).toContain("Claude · Sonnet");
    expect(notice?.textContent).toContain("GPT-5");

    await act(async () => {
      await useChat.getState().send({
        sessionId,
        cwd: "/tmp/tokage",
        text: "Continue with unit tests",
        mode: "direct",
        auto: false,
        ref: modelB,
        variant: { id: "gpt-5", effort: null, fast: false, tier: null },
        modelName: "GPT-5",
      });
    });

    expect(hoisted.startChatTurnMock).toHaveBeenCalledTimes(1);
    const call = hoisted.startChatTurnMock.mock.calls[0][0];
    expect(call.provider).toBe("CODEX_CLI");
    expect(call.model).toBe("gpt-5");
    expect(call.resume).toBeNull();
    // Direct switch replays full conversation history (excluding tokage system notices)
    expect(call.history).toHaveLength(2);
    expect(call.history[0].text).toContain("Design and implement the HandoffService API");
  });

  it("generates smart handoff via old model, allows free model switching while ready, and sends only handoff doc to new model", async () => {
    await useChat.getState().startSmartModelHandoff({
      sessionId,
      cwd: "/tmp/tokage",
      fromRef: modelA,
      fromModelName: "Claude Sonnet",
      toRef: modelB,
      toModelName: "GPT-5",
    });

    // 1. Old model A was called on its warm thread with phase=CONTEXT
    expect(hoisted.startChatTurnMock).toHaveBeenCalledTimes(1);
    const reportCall = hoisted.startChatTurnMock.mock.calls[0][0];
    expect(reportCall.provider).toBe("CLAUDE_CODE");
    expect(reportCall.phase).toBe("CONTEXT");
    expect(reportCall.resume).toBe("thread-claude-1");

    // 2. Simulate streaming and completion from old model A
    hoisted.chatEventHandler?.({
      turnId: reportCall.turnId,
      kind: "text",
      text: "## 1. Core Goal\nImplement HandoffService.\n## 2. Key Contracts\n`HandoffContract` interface.",
    });
    hoisted.chatEventHandler?.({
      turnId: reportCall.turnId,
      kind: "done",
      inputTokens: 100,
      outputTokens: 50,
    });

    const convAfterReport = useChat.getState().conversations[sessionId];
    expect(hasUnsentSmartHandoff(convAfterReport)).toBe(true);
    const readyMsg = getReadySmartHandoffMessage(convAfterReport);
    expect(readyMsg?.modelSwitch?.handoff?.status).toBe("ready");
    expect(readyMsg?.modelSwitch?.handoff?.source).toBe("model");
    expect(readyMsg?.modelSwitch?.handoff?.document).toContain("Implement HandoffService.");
    expect(readyMsg?.modelSwitch?.handoff?.document).toContain("src/handoff.ts");

    // 3. While ready, user freely switches target model from B (GPT-5) to C (Gemini Pro)
    useChat.getState().updateModelSwitchTarget(sessionId, modelC, "Gemini Pro");

    await renderConversation();
    expect(host.querySelector('[data-testid="handoff-ready-badge"]')).toBeTruthy();
    expect(host.querySelector('[data-testid="model-switch-notice"]')?.textContent).toContain("Gemini Pro");

    // Expand handoff detail in UI
    await act(async () => {
      (host.querySelector('[data-testid="smart-handoff-toggle"]') as HTMLButtonElement).click();
    });
    expect(host.querySelector('[data-testid="handoff-document-detail"]')?.textContent).toContain("HandoffContract");

    // 4. Send to new model C: sends ONLY handoff document + prompt in a fresh window (history = [])
    hoisted.startChatTurnMock.mockClear();
    await act(async () => {
      await useChat.getState().send({
        sessionId,
        cwd: "/tmp/tokage",
        text: "Please continue based on the handoff document.",
        mode: "direct",
        auto: false,
        ref: modelC,
        variant: { id: "gemini-pro", effort: null, fast: false, tier: null },
        modelName: "Gemini Pro",
      });
    });

    expect(hoisted.startChatTurnMock).toHaveBeenCalledTimes(1);
    const newModelCall = hoisted.startChatTurnMock.mock.calls[0][0];
    expect(newModelCall.provider).toBe("CURSOR_CLI");
    expect(newModelCall.model).toBe("gemini-pro");
    expect(newModelCall.resume).toBeNull();
    expect(newModelCall.history).toEqual([]);
    expect(newModelCall.prompt).toContain("[TOKAGE SESSION HANDOFF DOCUMENT]");
    expect(newModelCall.prompt).toContain("HandoffContract");
    expect(newModelCall.prompt).toContain("Please continue based on the handoff document.");

    const convAfterSend = useChat.getState().conversations[sessionId];
    expect(getReadySmartHandoffMessage(convAfterSend)).toBeNull();
  });

  it("falls back to Tokage local extraction and reminds user when old model fails", async () => {
    await useChat.getState().startSmartModelHandoff({
      sessionId,
      cwd: "/tmp/tokage",
      fromRef: modelA,
      fromModelName: "Claude Sonnet",
      toRef: modelB,
      toModelName: "GPT-5",
    });

    const reportCall = hoisted.startChatTurnMock.mock.calls[0][0];
    hoisted.chatEventHandler?.({
      turnId: reportCall.turnId,
      kind: "error",
      message: "429 Quota exceeded on Claude",
    });

    const readyMsg = getReadySmartHandoffMessage(useChat.getState().conversations[sessionId]);
    expect(readyMsg?.modelSwitch?.handoff?.status).toBe("ready");
    expect(readyMsg?.modelSwitch?.handoff?.source).toBe("tokage_local");
    expect(readyMsg?.modelSwitch?.handoff?.fallbackReason).toContain("429 Quota exceeded");

    await renderConversation();
    expect(host.querySelector('[data-testid="handoff-local-badge"]')).toBeTruthy();
    expect(host.querySelector('[data-testid="handoff-local-notice"]')?.textContent).toContain("429 Quota exceeded");

    await act(async () => {
      (host.querySelector('[data-testid="smart-handoff-toggle"]') as HTMLButtonElement).click();
    });
    expect(host.querySelector('[data-testid="handoff-document-detail"]')?.textContent).toContain("src/handoff.ts");
  });
});
