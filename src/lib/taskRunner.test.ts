import { describe, it, expect, vi, beforeEach } from "vitest";
import * as ipc from "./ipc";
import {
  resolveTaskProviderAndModel,
  buildSubAgentPrompt,
  handleSubAgentChatEvent,
  registerRunningSubAgent,
  resetSubAgentEventListener,
} from "./taskRunner";
import type { ProviderInfo, TaskItem } from "./ipc";
import { useExecution } from "../stores/execution";

describe("taskRunner", () => {
  const mockProviders: ProviderInfo[] = [
    {
      id: "CLAUDE_CODE",
      name: "Claude Code",
      status: "READY",
      chat: true,
      authMode: "OFFICIAL_AUTH",
      supportsSessionReuse: true,
      modelGroups: [
        {
          key: "claude-3-7-sonnet",
          name: "Claude 3.7 Sonnet",
          efforts: [],
          defaultEffort: null,
          contexts: [],
          thinking: false,
          fast: false,
          tags: [],
          tiers: [],
          defaultTier: null,
          variants: [
            {
              id: "sonnet",
              effort: null,
              context: null,
              thinking: false,
              fast: false,
            },
          ],
        },
      ],
      features: [],
      error: null,
    },
    {
      id: "CODEX_CLI",
      name: "Codex CLI",
      status: "READY",
      chat: true,
      authMode: "OFFICIAL_AUTH",
      supportsSessionReuse: true,
      modelGroups: [
        {
          key: "gpt-4o",
          name: "GPT-4o",
          efforts: [],
          defaultEffort: null,
          contexts: [],
          thinking: false,
          fast: false,
          tags: [],
          tiers: [],
          defaultTier: null,
          variants: [
            {
              id: "gpt-4o",
              effort: null,
              context: null,
              thinking: false,
              fast: false,
            },
          ],
        },
      ],
      features: [],
      error: null,
    },
  ];

  it("resolves assigned_model with explicit provider prefix", () => {
    const enabled = { CLAUDE_CODE: true, CODEX_CLI: true };
    const trust = { CLAUDE_CODE: true, CODEX_CLI: true };

    const resolved = resolveTaskProviderAndModel(
      "CLAUDE_CODE:sonnet",
      mockProviders,
      enabled,
      trust,
    );

    expect(resolved).not.toBeNull();
    expect(resolved?.provider).toBe("CLAUDE_CODE");
    expect(resolved?.model).toBe("sonnet");
  });

  it("resolves model by matching key across available providers", () => {
    const enabled = { CLAUDE_CODE: true, CODEX_CLI: true };
    const trust = { CLAUDE_CODE: true, CODEX_CLI: true };

    const resolved = resolveTaskProviderAndModel(
      "gpt-4o",
      mockProviders,
      enabled,
      trust,
    );

    expect(resolved).not.toBeNull();
    expect(resolved?.provider).toBe("CODEX_CLI");
    expect(resolved?.model).toBe("gpt-4o");
  });

  it("falls back to another runnable provider if the assigned provider is disabled", () => {
    const enabled = { CLAUDE_CODE: false, CODEX_CLI: true };
    const trust = { CLAUDE_CODE: true, CODEX_CLI: true };

    const resolved = resolveTaskProviderAndModel(
      "CLAUDE_CODE:sonnet",
      mockProviders,
      enabled,
      trust,
    );

    expect(resolved).not.toBeNull();
    expect(resolved?.provider).toBe("CODEX_CLI");
    expect(resolved?.fallbackUsed).toBe(true);
  });

  it("builds sub-agent prompt with task parameters and acceptance criteria", async () => {
    const task: TaskItem = {
      id: "T1",
      title: "实现用户模型",
      domain: "backend",
      goal: "创建 src/models/user.ts 并导出 User 接口",
      depends_on: [],
      contracts: [],
      write_scope: ["src/models/user.ts"],
      deliverables: ["src/models/user.ts"],
      acceptance: {
        commands: ["npm test user.test.ts"],
        criteria: ["类型定义完整"],
      },
      size: "M",
      difficulty: 2,
    };

    const prompt = await buildSubAgentPrompt(task);
    expect(prompt).toBeDefined();
    expect(typeof prompt).toBe("string");
    expect(prompt.length).toBeGreaterThan(0);
  });

  describe("m4 pipeline real-time streaming, gate execution, and DAG progression", () => {
    const sessionId = "session-test-m4";
    const t1: TaskItem = {
      id: "T1",
      title: "基础数据接口",
      domain: "backend",
      goal: "定义数据类型与存储结构",
      depends_on: [],
      contracts: [],
      write_scope: ["src/types.ts"],
      deliverables: ["src/types.ts"],
      acceptance: {
        commands: ["npm test types.test.ts"],
        criteria: ["类型检查通过"],
      },
      size: "S",
      difficulty: 1,
      assigned_model: "CLAUDE_CODE:sonnet",
    };

    const t2: TaskItem = {
      id: "T2",
      title: "用户业务逻辑",
      domain: "backend",
      goal: "基于 T1 的数据类型实现业务逻辑",
      depends_on: ["T1"],
      contracts: [],
      write_scope: ["src/user.ts"],
      deliverables: ["src/user.ts"],
      acceptance: {
        commands: ["npm test user.test.ts"],
        criteria: ["逻辑覆盖"],
      },
      size: "M",
      difficulty: 2,
      assigned_model: "CODEX_CLI:gpt-4o",
    };

    beforeEach(() => {
      resetSubAgentEventListener();
      useExecution.setState({ pipelines: {} });
      useExecution.getState().startPlanExecution(sessionId, [t1, t2], "/test/repo");
    });

    it("m4a: demuxes chat-event streams to task logs and updates changedFiles", async () => {
      const turnId = "subagent-session-test-m4-T1-test";
      const ctx = {
        sessionId,
        taskId: "T1",
        task: t1,
        worktreeCwd: "/test/repo",
        turnId,
        rawOutput: "",
        changedFiles: [],
        toolCalls: [],
        startTime: Date.now(),
      };
      registerRunningSubAgent(turnId, ctx);

      // 1. Tool start
      await handleSubAgentChatEvent({
        turnId,
        kind: "toolStart",
        id: "call-1",
        tool: "read",
        title: "package.json",
        detail: "Read dependencies",
      });

      // 2. Tool end
      await handleSubAgentChatEvent({
        turnId,
        kind: "toolEnd",
        id: "call-1",
        ok: true,
        output: "{}",
      });

      // 3. File change
      await handleSubAgentChatEvent({
        turnId,
        kind: "fileChange",
        toolId: "call-2",
        path: "src/types.ts",
        absPath: "/test/repo/src/types.ts",
        previewPath: null,
        change: "added",
        diff: "+export interface User {}",
        additions: 1,
        deletions: 0,
      });

      // 4. Streamed text
      await handleSubAgentChatEvent({
        turnId,
        kind: "text",
        text: "完成了 User 接口定义。",
      });

      expect(ctx.rawOutput).toBe("完成了 User 接口定义。");
      expect(ctx.changedFiles).toEqual(["src/types.ts"]);

      const logs = useExecution.getState().pipelines[sessionId].taskLogs["T1"];
      expect(logs.some((l) => l.includes("[工具调用: read] package.json"))).toBe(true);
      expect(logs.some((l) => l.includes("[工具完成: call-1] ✓ 成功"))).toBe(true);
      expect(logs.some((l) => l.includes("[文件变更] added: src/types.ts"))).toBe(true);
    });

    it("m4b & m4c: executes acceptance gate on completion, marks DONE, and triggers downstream task", async () => {
      const turnId = "subagent-session-test-m4-T1-gate-pass";
      const ctx = {
        sessionId,
        taskId: "T1",
        task: t1,
        worktreeCwd: "/test/repo",
        turnId,
        rawOutput: "代码已编写并通过本地自检。",
        changedFiles: ["src/types.ts"],
        toolCalls: [],
        startTime: Date.now(),
      };
      registerRunningSubAgent(turnId, ctx);

      // Mock executeGateCommand to return success
      const gateSpy = vi.spyOn(ipc, "executeGateCommand").mockResolvedValue({
        command: "npm test types.test.ts",
        passed: true,
        exitCode: 0,
        durationMs: 85,
        outputTail: "1 passed in 0.08s",
      });

      // Emit done event
      await handleSubAgentChatEvent({
        turnId,
        kind: "done",
        inputTokens: 100,
        outputTokens: 50,
        costUsd: 0.01,
      });

      expect(gateSpy).toHaveBeenCalledWith("npm test types.test.ts", "/test/repo", 300, true);

      const pipeline = useExecution.getState().pipelines[sessionId];

      // Gate result recorded
      expect(pipeline.gateResults).toHaveLength(1);
      expect(pipeline.gateResults[0]).toEqual({
        taskId: "T1",
        command: "npm test types.test.ts",
        passed: true,
        exitCode: 0,
        durationMs: 85,
        outputTail: "1 passed in 0.08s",
      });

      // T1 state is DONE
      expect(pipeline.taskStates["T1"]).toBe("DONE");

      // Upstream artifact recorded
      expect(pipeline.upstreamArtifacts?.["T1"]).toBeDefined();
      expect(pipeline.upstreamArtifacts?.["T1"].taskId).toBe("T1");

      // Downstream T2 dependencies were satisfied -> T2 advanced from PENDING to READY or RUNNING
      expect(pipeline.taskStates["T2"]).toBe("RUNNING");

      gateSpy.mockRestore();
    });

    it("m4b & m4c: marks task BLOCKED and does NOT advance downstream when gate fails", async () => {
      const turnId = "subagent-session-test-m4-T1-gate-fail";
      const ctx = {
        sessionId,
        taskId: "T1",
        task: t1,
        worktreeCwd: "/test/repo",
        turnId,
        rawOutput: "编写完成但有潜在类型错误。",
        changedFiles: ["src/types.ts"],
        toolCalls: [],
        startTime: Date.now(),
      };
      registerRunningSubAgent(turnId, ctx);

      // Mock executeGateCommand to return failure
      const gateSpy = vi.spyOn(ipc, "executeGateCommand").mockResolvedValue({
        command: "npm test types.test.ts",
        passed: false,
        exitCode: 1,
        durationMs: 120,
        outputTail: "FAIL: Type 'string' is not assignable to type 'number'",
      });

      await handleSubAgentChatEvent({
        turnId,
        kind: "done",
        inputTokens: 100,
        outputTokens: 50,
        costUsd: 0.01,
      });

      const pipeline = useExecution.getState().pipelines[sessionId];

      // T1 should be marked BLOCKED
      expect(pipeline.taskStates["T1"]).toBe("BLOCKED");

      // Gate failure logged in task logs
      const t1Logs = pipeline.taskLogs["T1"];
      expect(t1Logs.some((l) => l.includes("[验收失败]"))).toBe(true);

      // T2 must remain PENDING because T1 is BLOCKED
      expect(pipeline.taskStates["T2"]).toBe("PENDING");

      gateSpy.mockRestore();
    });

    it("marks task BLOCKED on subagent error event", async () => {
      const turnId = "subagent-session-test-m4-T1-error";
      const ctx = {
        sessionId,
        taskId: "T1",
        task: t1,
        worktreeCwd: "/test/repo",
        turnId,
        rawOutput: "",
        changedFiles: [],
        toolCalls: [],
        startTime: Date.now(),
      };
      registerRunningSubAgent(turnId, ctx);

      await handleSubAgentChatEvent({
        turnId,
        kind: "error",
        message: "CLI process crashed unexpectedly",
      });

      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.taskStates["T1"]).toBe("BLOCKED");
      expect(pipeline.taskLogs["T1"].some((l) => l.includes("CLI process crashed unexpectedly"))).toBe(true);
    });
  });
});
