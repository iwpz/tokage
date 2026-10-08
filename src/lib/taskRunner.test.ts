import { describe, it, expect, vi, beforeEach } from "vitest";
import * as ipc from "./ipc";
import {
  resolveTaskProviderAndModel,
  buildSubAgentPrompt,
  handleSubAgentChatEvent,
  registerRunningSubAgent,
  resetSubAgentEventListener,
  dispatchReadyTasks,
  getRunningSubAgent,
} from "./taskRunner";
import type { ProviderInfo, TaskItem } from "./ipc";
import { useExecution } from "../stores/execution";
import { useProviders } from "../stores/providers";
import { useSettings } from "../stores/settings";

describe("taskRunner", () => {
  const mockProviders: ProviderInfo[] = [
    {
      id: "CLAUDE_CODE",
      name: "Claude Code",
      status: "READY",
      chat: true,
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
  ] as unknown as ProviderInfo[];

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
        premiumRequests: null,
      });

      expect(gateSpy).toHaveBeenCalledWith("npm test types.test.ts", "/test/repo", 300, true);

      const pipeline = useExecution.getState().pipelines[sessionId];

      // Gate result recorded
      expect(pipeline.gateResults).toHaveLength(1);
      expect(pipeline.gateResults?.[0]).toEqual({
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
        premiumRequests: null,
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

    it("m5b: pause stops dispatching ready tasks and resume restarts downstream dispatch", async () => {
      const exec = useExecution.getState();
      // Set T1 to DONE
      exec.updateTaskState(sessionId, "T1", "DONE");
      // Pause pipeline
      exec.pauseExecution(sessionId);

      expect(useExecution.getState().pipelines[sessionId].status).toBe("PAUSED");

      // Attempt to progress downstream tasks while paused
      const { progressDownstreamTasks } = await import("./taskRunner");
      const launchedWhilePaused = await progressDownstreamTasks(sessionId);
      expect(launchedWhilePaused).toHaveLength(0);

      // Now resume pipeline
      exec.resumeExecution(sessionId);
      expect(useExecution.getState().pipelines[sessionId].status).toBe("RUNNING");

      // Spy on startChatTurn to verify execution
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      const launchedAfterResume = await progressDownstreamTasks(sessionId);
      expect(launchedAfterResume).toHaveLength(1);
      expect(launchedAfterResume[0]).toMatch(/^subagent-session-test-m4-T2-/);
      expect(useExecution.getState().pipelines[sessionId].taskStates["T2"]).toBe("RUNNING");

      chatSpy.mockRestore();
    });

    it("m5b: single task retry after BLOCKED with model reassignment", async () => {
      const exec = useExecution.getState();
      exec.updateTaskState(sessionId, "T1", "BLOCKED", "Gate failed");

      // Reassign model for T1
      exec.updateTaskAssignedModel(sessionId, "T1", "CLAUDE_CODE:sonnet");
      const updatedTask = useExecution.getState().pipelines[sessionId].tasks.find((t) => t.id === "T1");
      expect(updatedTask?.assigned_model).toBe("CLAUDE_CODE:sonnet");

      // Retry: BLOCKED → READY then claim (F1.h entry point)
      exec.updateTaskState(sessionId, "T1", "READY", "[重试] 重新排队");
      const { dispatchSubAgentTask } = await import("./taskRunner");
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);
      vi.spyOn(ipc, "createSessionWorktree").mockResolvedValue({
        name: "wt",
        cwd: "/test/repo/.tokage/worktrees/wt",
        branch: "tokage/wt",
      } as ipc.Worktree);

      const res = await dispatchSubAgentTask({
        sessionId,
        task: updatedTask!,
        contracts: [],
        upstreamArtifacts: [],
        baseCwd: "/test/repo",
        useWorktree: true,
      });

      expect(res.turnId).toMatch(/^subagent-session-test-m4-T1-/);
      expect(useExecution.getState().pipelines[sessionId].taskStates["T1"]).toBe("RUNNING");
      expect(useExecution.getState().pipelines[sessionId].taskLogs["T1"].some((l) => l.includes("sonnet"))).toBe(true);

      chatSpy.mockRestore();
    });

    it("m5c: cold start recovery correctly marks interrupted tasks and allows manual resume", async () => {
      const exec = useExecution.getState();
      const mockSavedState: ipc.PipelineState = {
        sessionId: "crash-session-99",
        status: "RUNNING",
        tasks: [
          {
            id: "T1",
            title: "Task 1",
            state: "RUNNING", // Was running when process crashed
            assignedModel: "sonnet",
          },
          {
            id: "T2",
            title: "Task 2",
            state: "PENDING",
            assignedModel: "gpt-4o",
          },
        ],
        taskStates: {
          T1: "RUNNING",
          T2: "PENDING",
        },
        taskLogs: {
          T1: ["[系统] 启动执行"],
          T2: [],
        },
        gateResults: [],
        startedAt: Date.now() - 50000,
        updatedAt: Date.now() - 10000,
      };

      vi.spyOn(ipc, "loadPipelineState").mockResolvedValue(mockSavedState);

      const recovered = await exec.loadPipelineFromDisk("/test/repo", "crash-session-99");
      expect(recovered).not.toBeNull();
      // Recovered session status should be INTERRUPTED
      expect(recovered?.status).toBe("INTERRUPTED");
      // Interrupted task should be marked INTERRUPTED
      expect(recovered?.taskStates["T1"]).toBe("INTERRUPTED");
      // Logs should contain recovery warning
      expect(recovered?.taskLogs["T1"].some((l) => l.includes("异常退出/崩溃中断"))).toBe(true);
    });

    it("m5c: E2E multi-task DAG chain execution (T1 -> Gate -> T2)", async () => {
      const e2eSession = "e2e-dag-session";
      const exec = useExecution.getState();

      const taskA: TaskItem = {
        id: "TA",
        title: "创建共享工具模块",
        domain: "backend",
        goal: "创建 src/utils.ts 并导出 helper 函数",
        depends_on: [],
        contracts: [],
        write_scope: ["src/utils.ts"],
        deliverables: ["src/utils.ts"],
        acceptance: { commands: ["npm test utils.test.ts"], criteria: [] },
        size: "S",
        difficulty: 1,
        assigned_model: "CLAUDE_CODE:sonnet",
      };

      const taskB: TaskItem = {
        id: "TB",
        title: "编写调用工具模块的业务逻辑",
        domain: "backend",
        goal: "使用 src/utils.ts 并完成业务",
        depends_on: ["TA"],
        contracts: [],
        write_scope: ["src/biz.ts"],
        deliverables: ["src/biz.ts"],
        acceptance: { commands: ["npm test biz.test.ts"], criteria: [] },
        size: "M",
        difficulty: 2,
        assigned_model: "CODEX_CLI:gpt-4o",
      };

      // 1. Start execution — roots are READY until dispatcher claims them (F1.b)
      exec.startPlanExecution(e2eSession, [taskA, taskB], "/test/repo");
      expect(useExecution.getState().pipelines[e2eSession].taskStates["TA"]).toBe("READY");
      expect(useExecution.getState().pipelines[e2eSession].taskStates["TB"]).toBe("PENDING");

      // Claim TA as RUNNING (simulates dispatchSubAgentTask) before synthesizing done
      exec.updateTaskState(e2eSession, "TA", "RUNNING", "[test] claimed", "subagent-ta-turn");
      const turnA = "subagent-ta-turn";
      registerRunningSubAgent(turnA, {
        sessionId: e2eSession,
        taskId: "TA",
        task: taskA,
        worktreeCwd: "/test/repo",
        turnId: turnA,
        rawOutput: "utils.ts 已创建完成。",
        changedFiles: ["src/utils.ts"],
        toolCalls: [],
        startTime: Date.now(),
      });

      const gateSpy = vi.spyOn(ipc, "executeGateCommand").mockResolvedValue({
        command: "npm test utils.test.ts",
        passed: true,
        exitCode: 0,
        durationMs: 90,
        outputTail: "TA passed",
      });

      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      // TA emits done event -> verifies gate -> marks DONE -> unblocks TB -> launches TB
      await handleSubAgentChatEvent({
        turnId: turnA,
        kind: "done",
        inputTokens: 100,
        outputTokens: 50,
        costUsd: 0.02,
        premiumRequests: null,
      });

      const pipeline = useExecution.getState().pipelines[e2eSession];
      expect(pipeline.taskStates["TA"]).toBe("DONE");
      expect(pipeline.upstreamArtifacts?.["TA"]).toBeDefined();
      expect(pipeline.upstreamArtifacts?.["TA"].changedFiles).toContain("src/utils.ts");

      // TB should now be launched and RUNNING
      expect(pipeline.taskStates["TB"]).toBe("RUNNING");
      expect(pipeline.taskLogs["TB"].some((l) => l.includes("启动子代理进程"))).toBe(true);

      gateSpy.mockRestore();
      chatSpy.mockRestore();
    });
  });

  describe("F1: first dispatch after plan approval", () => {
    const sessionId = "f1-approve-dispatch";
    const mockProviders: ProviderInfo[] = [
      {
        id: "CLAUDE_CODE",
        name: "Claude Code",
        status: "READY",
        chat: true,
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
    ] as unknown as ProviderInfo[];

    beforeEach(() => {
      resetSubAgentEventListener();
      useExecution.setState({ pipelines: {} });
      useProviders.setState({ providers: mockProviders, scanning: false, pending: [], lastScan: null, error: null });
      useSettings.setState({
        providerEnabled: { CLAUDE_CODE: true },
        providerTrust: { CLAUDE_CODE: true },
        isolation: "inplace",
        maxParallel: 3,
      });
    });

    it("F1.a: dispatchReadyTasks launches root task after startPlanExecution", async () => {
      const t1: TaskItem = {
        id: "T1",
        title: "根任务",
        domain: "backend",
        goal: "先做 T1",
        depends_on: [],
        contracts: [],
        write_scope: ["src/a.ts"],
        deliverables: ["src/a.ts"],
        acceptance: { commands: [], criteria: [] },
        size: "S",
        difficulty: 1,
        assigned_model: "CLAUDE_CODE:sonnet",
      };
      const t2: TaskItem = {
        id: "T2",
        title: "下游任务",
        domain: "backend",
        goal: "依赖 T1",
        depends_on: ["T1"],
        contracts: [],
        write_scope: ["src/b.ts"],
        deliverables: ["src/b.ts"],
        acceptance: { commands: [], criteria: [] },
        size: "S",
        difficulty: 1,
        assigned_model: "CLAUDE_CODE:sonnet",
      };

      useExecution.getState().startPlanExecution(sessionId, [t1, t2], "/test/repo");
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      const launched = await dispatchReadyTasks(sessionId);

      expect(chatSpy).toHaveBeenCalledTimes(1);
      expect(launched).toHaveLength(1);
      expect(launched[0]).toMatch(new RegExp(`^subagent-${sessionId}-T1-`));
      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.taskStates["T1"]).toBe("RUNNING");
      expect(pipeline.taskStates["T2"]).toBe("PENDING");

      chatSpy.mockRestore();
    });

    it("F1.c: concurrent dispatchReadyTasks claims each root task once", async () => {
      const t1: TaskItem = {
        id: "T1",
        title: "根任务",
        domain: "backend",
        goal: "T1",
        depends_on: [],
        contracts: [],
        write_scope: ["src/a.ts"],
        deliverables: ["src/a.ts"],
        acceptance: { commands: [], criteria: [] },
        size: "S",
        difficulty: 1,
        assigned_model: "CLAUDE_CODE:sonnet",
      };
      useExecution.getState().startPlanExecution(sessionId, [t1], "/test/repo");
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      const [a, b] = await Promise.all([
        dispatchReadyTasks(sessionId),
        dispatchReadyTasks(sessionId),
      ]);
      const all = [...a, ...b];
      expect(chatSpy).toHaveBeenCalledTimes(1);
      expect(all).toHaveLength(1);
      expect(useExecution.getState().pipelines[sessionId].taskStates["T1"]).toBe("RUNNING");

      chatSpy.mockRestore();
    });

    it("F1.c: startChatTurn failure BLOCKs and unregisters ctx", async () => {
      const t1: TaskItem = {
        id: "T1",
        title: "根任务",
        domain: "backend",
        goal: "T1",
        depends_on: [],
        contracts: [],
        write_scope: ["src/a.ts"],
        deliverables: ["src/a.ts"],
        acceptance: { commands: [], criteria: [] },
        size: "S",
        difficulty: 1,
        assigned_model: "CLAUDE_CODE:sonnet",
      };
      useExecution.getState().startPlanExecution(sessionId, [t1], "/test/repo");
      const chatSpy = vi
        .spyOn(ipc, "startChatTurn")
        .mockRejectedValue(new Error("boom"));

      const launched = await dispatchReadyTasks(sessionId);
      expect(launched).toHaveLength(0);
      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.taskStates["T1"]).toBe("BLOCKED");
      const turnId = pipeline.taskTurnIds?.["T1"];
      expect(turnId).toBeTruthy();
      expect(getRunningSubAgent(turnId!)).toBeUndefined();

      chatSpy.mockRestore();
    });

    it("F1.c: worktree failure BLOCKs and unregisters ctx", async () => {
      useSettings.setState({ isolation: "worktree" });
      const t1: TaskItem = {
        id: "T1",
        title: "根任务",
        domain: "backend",
        goal: "T1",
        depends_on: [],
        contracts: [],
        write_scope: ["src/a.ts"],
        deliverables: ["src/a.ts"],
        acceptance: { commands: [], criteria: [] },
        size: "S",
        difficulty: 1,
        assigned_model: "CLAUDE_CODE:sonnet",
      };
      useExecution.getState().startPlanExecution(sessionId, [t1], "/test/repo");
      const wtSpy = vi
        .spyOn(ipc, "createSessionWorktree")
        .mockRejectedValue(new Error("wt failed"));
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      const launched = await dispatchReadyTasks(sessionId);
      expect(launched).toHaveLength(0);
      expect(chatSpy).not.toHaveBeenCalled();
      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.taskStates["T1"]).toBe("BLOCKED");
      expect(getRunningSubAgent(pipeline.taskTurnIds?.["T1"] ?? "")).toBeUndefined();

      wtSpy.mockRestore();
      chatSpy.mockRestore();
    });
  });
});
