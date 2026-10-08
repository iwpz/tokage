import { describe, it, expect, vi, beforeEach } from "vitest";
import * as ipc from "./ipc";
import {
  resolveTaskProviderAndModel,
  buildSubAgentPrompt,
  handleSubAgentChatEvent,
  handleSubAgentChatEnded,
  registerRunningSubAgent,
  resetSubAgentEventListener,
  dispatchReadyTasks,
  confirmContractDeviation,
  formatTaskWaitReason,
  getRunningSubAgent,
  cancelTask,
  continueTask,
  resolvePipelineTaskApproval,
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

  it("returns null when the assigned provider is disabled (no silent fallback)", () => {
    const enabled = { CLAUDE_CODE: false, CODEX_CLI: true };
    const trust = { CLAUDE_CODE: true, CODEX_CLI: true };

    const resolved = resolveTaskProviderAndModel(
      "CLAUDE_CODE:sonnet",
      mockProviders,
      enabled,
      trust,
    );

    expect(resolved).toBeNull();
  });

  it("returns null for unknown model under an explicit provider", () => {
    const enabled = { CLAUDE_CODE: true };
    const trust = { CLAUDE_CODE: true };
    expect(
      resolveTaskProviderAndModel("CLAUDE_CODE:not-a-real-model", mockProviders, enabled, trust),
    ).toBeNull();
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
      useProviders.setState({
        providers: mockProviders,
        scanning: false,
        pending: [],
        lastScan: null,
        error: null,
      });
      useSettings.setState({
        providerEnabled: { CLAUDE_CODE: true, CODEX_CLI: true },
        providerTrust: { CLAUDE_CODE: true, CODEX_CLI: true },
        isolation: "inplace",
        maxParallel: 3,
        agentIdleTimeoutMin: 30,
        agentHardTimeoutMin: 120,
      });
      useExecution.getState().startPlanExecution(sessionId, [t1, t2], "/test/repo");
    });

    function claimRunning(taskId: string, turnId: string) {
      const ok = useExecution.getState().claimTaskRunning(sessionId, taskId, turnId, "[test] claim");
      expect(ok).toBe(true);
    }

    it("m4a: demuxes chat-event streams to task logs and updates changedFiles", async () => {
      const turnId = "subagent-session-test-m4-T1-test";
      claimRunning("T1", turnId);
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
      claimRunning("T1", turnId);
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
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      // Emit done event
      await handleSubAgentChatEvent({
        turnId,
        kind: "done",
        inputTokens: 100,
        outputTokens: 50,
        costUsd: 0.01,
        premiumRequests: null,
      });

      expect(gateSpy).toHaveBeenCalledWith(
        "npm test types.test.ts",
        "/test/repo",
        300,
        false,
        expect.any(Array),
        expect.stringMatching(/^gate:/),
      );

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
      chatSpy.mockRestore();
    });

    it("F5.i: out-of-scope changed files BLOCK the task, skip gate, and record evidence", async () => {
      const turnId = "subagent-session-test-m4-T1-out-of-scope";
      claimRunning("T1", turnId);
      const ctx = {
        sessionId,
        taskId: "T1",
        task: t1,
        worktreeCwd: "/test/repo",
        turnId,
        rawOutput: "done",
        changedFiles: ["src/types.ts", "src/other.ts"],
        toolCalls: [],
        startTime: Date.now(),
      };
      registerRunningSubAgent(turnId, ctx);

      vi.spyOn(ipc, "validateWriteScope").mockImplementation(async (scopes, files) => {
        const allowedFiles = files.filter((f) => scopes.includes(f));
        const outOfScopeFiles = files.filter((f) => !scopes.includes(f));
        return {
          passed: outOfScopeFiles.length === 0,
          declaredScopes: scopes,
          changedFiles: files,
          allowedFiles,
          outOfScopeFiles,
        };
      });
      const gateSpy = vi.spyOn(ipc, "executeGateCommand");
      gateSpy.mockClear();

      await handleSubAgentChatEvent({
        turnId,
        kind: "done",
        inputTokens: 1,
        outputTokens: 1,
        costUsd: 0,
        premiumRequests: null,
      });

      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(gateSpy).not.toHaveBeenCalled();
      expect(pipeline.taskStates["T1"]).toBe("BLOCKED");
      expect(pipeline.taskFailureEvidence?.["T1"]?.outOfScopeFiles).toEqual(["src/other.ts"]);
      expect(pipeline.taskFailureEvidence?.["T1"]?.workdir).toBe("/test/repo");
      expect(pipeline.taskLogs["T1"].some((l) => l.includes("[写入越界]") && l.includes("src/other.ts"))).toBe(true);
    });

    it("m4b & m4c: marks task BLOCKED and does NOT advance downstream when gate fails", async () => {
      const turnId = "subagent-session-test-m4-T1-gate-fail";
      claimRunning("T1", turnId);
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
      claimRunning("T1", turnId);
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

    it("F3.e: idle/hard timeout errors BLOCK with typed reason and evidence", async () => {
      useSettings.setState({ agentIdleTimeoutMin: 30, agentHardTimeoutMin: 120 });
      const turnId = "subagent-session-test-m4-T1-timeout";
      claimRunning("T1", turnId);
      registerRunningSubAgent(turnId, {
        sessionId,
        taskId: "T1",
        task: t1,
        worktreeCwd: "/test/repo",
        turnId,
        rawOutput: "partial work before hang",
        changedFiles: ["src/types.ts"],
        toolCalls: [],
        startTime: Date.now(),
      });

      await handleSubAgentChatEvent({
        turnId,
        kind: "error",
        message: "Command timed out (idle timeout exceeded)",
      });

      const pipe = useExecution.getState().pipelines[sessionId];
      expect(pipe.taskStates["T1"]).toBe("BLOCKED");
      expect(pipe.taskLogs["T1"].some((l) => l.includes("空闲超时（30 分钟）"))).toBe(true);
      expect(pipe.taskLogs["T1"].some((l) => l.includes("src/types.ts"))).toBe(true);
      expect(pipe.taskLogs["T1"].some((l) => l.includes("partial work before hang"))).toBe(true);
    });

    it("m5b: pause stops dispatching ready tasks and resume restarts downstream dispatch", async () => {
      const exec = useExecution.getState();
      // Legal path to DONE: READY → RUNNING → VERIFYING → DONE
      claimRunning("T1", "subagent-pause-t1");
      exec.updateTaskState(sessionId, "T1", "VERIFYING");
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

    it("F3.d: cancelTask cancels RUNNING, skips downstream, ignores late done", async () => {
      const turnId = "subagent-session-test-m4-T1-cancel";
      claimRunning("T1", turnId);
      registerRunningSubAgent(turnId, {
        sessionId,
        taskId: "T1",
        task: t1,
        worktreeCwd: "/test/repo",
        turnId,
        rawOutput: "",
        changedFiles: [],
        toolCalls: [],
        startTime: Date.now(),
      });
      const cancelSpy = vi.spyOn(ipc, "cancelChatTurn").mockResolvedValue(undefined);
      const gateSpy = vi.spyOn(ipc, "executeGateCommand").mockResolvedValue({
        passed: true,
        exitCode: 0,
        durationMs: 1,
        outputTail: "ok",
        errorMessage: null,
      } as ipc.GateCommandResult);

      await cancelTask(sessionId, "T1");

      expect(cancelSpy).toHaveBeenCalledWith(turnId);
      const pipe = useExecution.getState().pipelines[sessionId];
      expect(pipe.taskStates["T1"]).toBe("CANCELLED");
      expect(pipe.taskStates["T2"]).toBe("SKIPPED");
      expect(getRunningSubAgent(turnId)).toBeUndefined();

      await handleSubAgentChatEvent({
        turnId,
        kind: "done",
        inputTokens: 1,
        outputTokens: 1,
        costUsd: null,
        premiumRequests: null,
      });
      expect(useExecution.getState().pipelines[sessionId].taskStates["T1"]).toBe("CANCELLED");
      expect(gateSpy).not.toHaveBeenCalled();

      cancelSpy.mockRestore();
      gateSpy.mockRestore();
    });

    it("F3.c: dispatchSubAgentTask passes AGENT idle/hard secs from settings", async () => {
      useSettings.setState({ agentIdleTimeoutMin: 15, agentHardTimeoutMin: 45 });
      useExecution.getState().updateTaskState(sessionId, "T1", "READY", "[test] requeue");
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);
      const { dispatchSubAgentTask } = await import("./taskRunner");
      await dispatchSubAgentTask({
        sessionId,
        task: t1,
        contracts: [],
        upstreamArtifacts: [],
        baseCwd: "/test/repo",
        useWorktree: false,
      });
      expect(chatSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          timeoutTier: "AGENT",
          idleTimeoutSecs: 15 * 60,
          hardTimeoutSecs: 45 * 60,
        }),
      );
      chatSpy.mockRestore();
    });

    it("m5b: single task retry after BLOCKED with model reassignment", async () => {
      const exec = useExecution.getState();
      claimRunning("T1", "subagent-retry-setup");
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
        root: "/test/repo/.tokage/worktrees/wt",
        cwd: "/test/repo/.tokage/worktrees/wt",
        branch: "tokage/wt",
      });

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
      // F7.e: Rust load_pipeline_state already rewrote RUNNING → INTERRUPTED.
      const mockSavedState: ipc.PipelineState = {
        schemaVersion: 2,
        sessionId: "crash-session-99",
        status: "INTERRUPTED",
        baseCwd: "/test/repo",
        tasks: [
          {
            id: "T1",
            title: "Task 1",
            state: "INTERRUPTED",
            assignedModel: "CLAUDE_CODE:sonnet",
            cwd: "/test/repo/wt-t1",
          },
          {
            id: "T2",
            title: "Task 2",
            state: "PENDING",
            assignedModel: "CODEX_CLI:gpt-4o",
          },
        ],
        taskItems: [
          {
            id: "T1",
            title: "Task 1",
            domain: "backend",
            goal: "g1",
            depends_on: [],
            contracts: [],
            write_scope: ["src/a.ts"],
            deliverables: ["src/a.ts"],
            acceptance: { commands: [], criteria: [] },
            size: "S",
            difficulty: 1,
            assigned_model: "CLAUDE_CODE:sonnet",
          },
          {
            id: "T2",
            title: "Task 2",
            domain: "backend",
            goal: "g2",
            depends_on: ["T1"],
            contracts: [],
            write_scope: ["src/b.ts"],
            deliverables: ["src/b.ts"],
            acceptance: { commands: [], criteria: [] },
            size: "S",
            difficulty: 1,
            assigned_model: "CODEX_CLI:gpt-4o",
          },
        ],
        taskStates: {
          T1: "INTERRUPTED",
          T2: "PENDING",
        },
        taskLogs: {
          T1: ["[系统] 启动执行", "[系统] 检测到先前执行异常中断"],
          T2: [],
        },
        gateResults: [],
        startedAt: Date.now() - 50000,
        updatedAt: Date.now() - 10000,
      };

      vi.spyOn(ipc, "loadPipelineState").mockResolvedValue(mockSavedState);

      const recovered = await exec.loadPipelineFromDisk("/test/repo", "crash-session-99");
      expect(recovered).not.toBeNull();
      expect(recovered?.status).toBe("INTERRUPTED");
      expect(recovered?.taskStates["T1"]).toBe("INTERRUPTED");
      expect(recovered?.tasks[0].depends_on).toEqual([]);
      expect(recovered?.taskCwds?.["T1"]).toBe("/test/repo/wt-t1");
      expect(recovered?.taskLogs["T1"].some((l) => l.includes("异常中断"))).toBe(true);
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
      const turnA = "subagent-ta-turn";
      expect(exec.claimTaskRunning(e2eSession, "TA", turnA, "[test] claimed")).toBe(true);
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

    it("F1.h: dispatchReadyTasks({ only }) retries a single READY task", async () => {
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
      const t2: TaskItem = {
        ...t1,
        id: "T2",
        title: "另一根任务",
        write_scope: ["src/b.ts"],
        deliverables: ["src/b.ts"],
      };
      useExecution.getState().startPlanExecution(sessionId, [t1, t2], "/test/repo");
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      const launched = await dispatchReadyTasks(sessionId, { only: ["T2"] });
      expect(launched).toHaveLength(1);
      expect(launched[0]).toMatch(new RegExp(`^subagent-${sessionId}-T2-`));
      expect(useExecution.getState().pipelines[sessionId].taskStates["T1"]).toBe("READY");
      expect(useExecution.getState().pipelines[sessionId].taskStates["T2"]).toBe("RUNNING");
      expect(chatSpy).toHaveBeenCalledTimes(1);

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

  describe("F9: Rust DagScheduler pick wiring", () => {
    const sessionId = "session-test-f9";

    const rootTask = (id: string, scope: string[]): TaskItem => ({
      id,
      title: id,
      domain: "backend",
      goal: id,
      depends_on: [],
      contracts: [],
      write_scope: scope,
      deliverables: scope,
      acceptance: { commands: [], criteria: [] },
      size: "S",
      difficulty: 1,
      assigned_model: "CLAUDE_CODE:sonnet",
    });

    beforeEach(() => {
      resetSubAgentEventListener();
      useExecution.setState({ pipelines: {} });
      useProviders.setState({
        providers: mockProviders,
        scanning: false,
        pending: [],
      });
      useSettings.setState({
        providerEnabled: { CLAUDE_CODE: true },
        providerTrust: { CLAUDE_CODE: true },
        isolation: "inplace",
        maxParallel: 3,
      });
    });

    it("F9.a: applies promoted, launches only dispatchable, respects opts.only ∩ dispatchable", async () => {
      const t1 = rootTask("T1", ["src/a.ts"]);
      const t2: TaskItem = {
        ...rootTask("T2", ["src/b.ts"]),
        depends_on: ["T1"],
      };
      const t3 = rootTask("T3", ["src/c.ts"]);
      useExecution.getState().startPlanExecution(sessionId, [t1, t2, t3], "/test/repo");
      // Simulate T1 already DONE so T2 can be promoted
      useExecution.getState().updateTaskState(sessionId, "T1", "RUNNING");
      useExecution.getState().updateTaskState(sessionId, "T1", "VERIFYING");
      useExecution.getState().updateTaskState(sessionId, "T1", "DONE");

      const pickSpy = vi.spyOn(ipc, "pickDispatchableTasks").mockResolvedValue({
        promoted: ["T2"],
        dispatchable: ["T2", "T3"],
        ineligible: {
          // not actually checked for launch; stored for wait UX
        },
      });
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      const launched = await dispatchReadyTasks(sessionId, { only: ["T2"] });
      expect(pickSpy).toHaveBeenCalled();
      expect(launched).toHaveLength(1);
      expect(launched[0]).toMatch(new RegExp(`^subagent-${sessionId}-T2-`));
      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.taskStates["T2"]).toBe("RUNNING");
      expect(pipeline.taskStates["T3"]).toBe("READY"); // not in only ∩ dispatchable
      expect(chatSpy).toHaveBeenCalledTimes(1);

      pickSpy.mockRestore();
      chatSpy.mockRestore();
    });

    it("F9.a: stores ineligible as taskWaitReasons", async () => {
      const t1 = rootTask("T1", ["src/a.ts"]);
      const t2 = rootTask("T2", ["src/b.ts"]);
      useExecution.getState().startPlanExecution(sessionId, [t1, t2], "/test/repo");
      const reason = {
        writeScopeCollision: {
          conflictingTaskId: "T1",
          overlappingPaths: ["src/**", "src/b.ts"],
        },
      } as const;
      const pickSpy = vi.spyOn(ipc, "pickDispatchableTasks").mockResolvedValue({
        promoted: [],
        dispatchable: ["T1"],
        ineligible: { T2: reason },
      });
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      await dispatchReadyTasks(sessionId);
      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.taskWaitReasons?.["T2"]).toEqual(reason);
      expect(formatTaskWaitReason(reason)).toContain("T1");

      pickSpy.mockRestore();
      chatSpy.mockRestore();
    });

    it("F9.e: deviation source blocks downstream until confirmed", async () => {
      const t1 = rootTask("T1", ["src/a.ts"]);
      const t2: TaskItem = {
        ...rootTask("T2", ["src/b.ts"]),
        depends_on: ["T1"],
      };
      useExecution.getState().startPlanExecution(sessionId, [t1, t2], "/test/repo");
      useExecution.getState().updateTaskState(sessionId, "T1", "RUNNING");
      useExecution.getState().updateTaskState(sessionId, "T1", "VERIFYING");
      useExecution.getState().updateTaskState(sessionId, "T1", "DONE");
      useExecution.getState().addDeviationSource(sessionId, "T1");

      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);
      // Use real browser mock of pickDispatchableTasks (no spy) — respects deviationSources
      const launchedBlocked = await dispatchReadyTasks(sessionId);
      expect(launchedBlocked).toHaveLength(0);
      expect(useExecution.getState().pipelines[sessionId].taskStates["T2"]).toBe("READY");
      expect(
        useExecution.getState().pipelines[sessionId].taskWaitReasons?.["T2"],
      ).toMatchObject({ contractDeviationPaused: { sourceTaskId: "T1" } });

      const launchedAfter = await confirmContractDeviation(sessionId, "T1");
      expect(useExecution.getState().pipelines[sessionId].deviationSources).toEqual([]);
      expect(launchedAfter).toHaveLength(1);
      expect(useExecution.getState().pipelines[sessionId].taskStates["T2"]).toBe("RUNNING");

      chatSpy.mockRestore();
    });
  });

  describe("F6: sub-agent events and approvals", () => {
    const sessionId = "session-test-f6";
    const t1: TaskItem = {
      id: "T1",
      title: "敏感命令任务",
      domain: "backend",
      goal: "需要审批的命令",
      depends_on: [],
      contracts: [],
      write_scope: ["src/a.ts"],
      deliverables: ["src/a.ts"],
      acceptance: {
        commands: ["npm test"],
        criteria: ["通过"],
        confirmedCommands: ["npm test"],
      },
      size: "S",
      difficulty: 1,
      assigned_model: "CLAUDE_CODE:sonnet",
    };

    beforeEach(() => {
      resetSubAgentEventListener();
      useExecution.setState({ pipelines: {} });
      useProviders.setState({
        providers: mockProviders,
        scanning: false,
        pending: [],
        lastScan: null,
        error: null,
      });
      useSettings.setState({
        providerEnabled: { CLAUDE_CODE: true, CODEX_CLI: true },
        providerTrust: { CLAUDE_CODE: true, CODEX_CLI: true },
        isolation: "inplace",
        maxParallel: 3,
        agentIdleTimeoutMin: 30,
        agentHardTimeoutMin: 120,
      });
      useExecution.getState().startPlanExecution(sessionId, [t1], "/test/repo");
    });

    function claimRunning(turnId: string) {
      const ok = useExecution.getState().claimTaskRunning(sessionId, "T1", turnId, "[test] claim");
      expect(ok).toBe(true);
      registerRunningSubAgent(turnId, {
        sessionId,
        taskId: "T1",
        task: t1,
        worktreeCwd: "/test/repo",
        turnId,
        rawOutput: "",
        changedFiles: [],
        toolCalls: [],
        startTime: Date.now(),
      });
    }

    it("F6.a: thread / quota / approvalRequest / reasoning update store and logs", async () => {
      const turnId = "subagent-f6-a";
      claimRunning(turnId);

      await handleSubAgentChatEvent({ turnId, kind: "thread", id: "cli-thread-99" });
      await handleSubAgentChatEvent({
        turnId,
        kind: "quota",
        raw: "rate_limit",
        resetAt: "soon",
        retrying: true,
      });
      await handleSubAgentChatEvent({
        turnId,
        kind: "reasoning",
        text: "I should run rm -rf build\nmore thoughts",
      });
      await handleSubAgentChatEvent({
        turnId,
        kind: "approvalRequest",
        id: "apr_1",
        toolId: "call_1",
        command: "rm -rf build",
        level: "sensitive",
        reason: "disallowed_tool",
        recoverable: null,
      });

      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.threadIds?.["T1"]).toBe("cli-thread-99");
      expect(pipeline.taskApprovals?.["T1"]).toHaveLength(1);
      expect(pipeline.taskApprovals?.["T1"][0].command).toBe("rm -rf build");
      expect(pipeline.taskStates["T1"]).toBe("RUNNING");
      const logs = pipeline.taskLogs["T1"].join("\n");
      expect(logs).toContain("[会话] CLI thread=cli-thread-99");
      expect(logs).toContain("[额度] rate_limit");
      expect(logs).toContain("[思考]");
      expect(logs).toContain("[待审批] rm -rf build");
    });

    it("F6.b: approvalRequest then chat-ended (no done) → BLOCKED, no gate", async () => {
      const turnId = "subagent-f6-b-ended";
      claimRunning(turnId);
      const gateSpy = vi.spyOn(ipc, "executeGateCommand");

      await handleSubAgentChatEvent({
        turnId,
        kind: "approvalRequest",
        id: "apr_ended",
        toolId: "call_1",
        command: "rm -rf build",
        level: "sensitive",
        reason: "disallowed_tool",
        recoverable: null,
      });
      await handleSubAgentChatEnded({ turnId, cancelled: false });

      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.taskStates["T1"]).toBe("BLOCKED");
      expect(pipeline.taskLogs["T1"].join("\n")).toContain("[等待审批]");
      expect(gateSpy).not.toHaveBeenCalled();
      expect(getRunningSubAgent(turnId)).toBeUndefined();
      gateSpy.mockRestore();
    });

    it("F6.b: approvalRequest then done → BLOCKED, no gate", async () => {
      const turnId = "subagent-f6-b-done";
      claimRunning(turnId);
      const gateSpy = vi.spyOn(ipc, "executeGateCommand");

      await handleSubAgentChatEvent({
        turnId,
        kind: "approvalRequest",
        id: "apr_done",
        toolId: "call_1",
        command: "rm -rf build",
        level: "sensitive",
        reason: "disallowed_tool",
        recoverable: null,
      });
      await handleSubAgentChatEvent({
        turnId,
        kind: "done",
        inputTokens: 1,
        outputTokens: 1,
        costUsd: null,
        premiumRequests: null,
      });

      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.taskStates["T1"]).toBe("BLOCKED");
      expect(pipeline.taskLogs["T1"].join("\n")).toContain("[等待审批]");
      expect(gateSpy).not.toHaveBeenCalled();
      gateSpy.mockRestore();
    });

    it("F6.b: chat-ended without done/error/approval → BLOCKED abnormal exit", async () => {
      const turnId = "subagent-f6-b-abnormal";
      claimRunning(turnId);
      await handleSubAgentChatEnded({ turnId, cancelled: false });
      const pipeline = useExecution.getState().pipelines[sessionId];
      expect(pipeline.taskStates["T1"]).toBe("BLOCKED");
      expect(pipeline.taskLogs["T1"].join("\n")).toContain("CLI 异常退出（无终止事件）");
    });

    it("F6.c: approve resumes with resume/sessionAllowed/EXECUTOR; deny uses deny prompt", async () => {
      const turnId = "subagent-f6-c";
      claimRunning(turnId);
      useExecution.getState().setTaskThreadId(sessionId, "T1", "cli-thread-resume");

      await handleSubAgentChatEvent({
        turnId,
        kind: "approvalRequest",
        id: "apr_resume",
        toolId: "call_1",
        command: "rm -rf build",
        level: "sensitive",
        reason: "disallowed_tool",
        recoverable: null,
      });
      await handleSubAgentChatEnded({ turnId, cancelled: false });
      expect(useExecution.getState().pipelines[sessionId].taskStates["T1"]).toBe("BLOCKED");

      const resolveSpy = vi.spyOn(ipc, "resolveApproval").mockResolvedValue(undefined);
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      await resolvePipelineTaskApproval(sessionId, "T1", "apr_resume", "allow_session");

      expect(resolveSpy).toHaveBeenCalled();
      expect(chatSpy).toHaveBeenCalledTimes(1);
      const req = chatSpy.mock.calls[0][0];
      expect(req.resume).toBe("cli-thread-resume");
      expect(req.sessionAllowed).toContain("rm -rf build");
      expect(req.role).toBe("EXECUTOR");
      expect(req.prompt).toContain("User approved command: rm -rf build");
      expect(useExecution.getState().pipelines[sessionId].taskStates["T1"]).toBe("RUNNING");
      expect(useExecution.getState().pipelines[sessionId].taskAllowed?.["T1"]).toContain(
        "rm -rf build",
      );

      // Deny path on a fresh approval after re-blocking
      resetSubAgentEventListener();
      useExecution.getState().updateTaskState(sessionId, "T1", "BLOCKED", "[test] reblock");
      useExecution.getState().upsertTaskApproval(sessionId, "T1", {
        id: "apr_deny",
        turnId: "old",
        toolId: "call_2",
        command: "curl evil",
        level: "sensitive",
        reason: "disallowed_tool",
        recoverable: null,
        resolved: false,
      });
      chatSpy.mockClear();
      await resolvePipelineTaskApproval(sessionId, "T1", "apr_deny", "deny");
      expect(chatSpy.mock.calls[0][0].prompt).toContain("User denied command: curl evil");

      resolveSpy.mockRestore();
      chatSpy.mockRestore();
    });
  });

  describe("F7: breakpoint resume", () => {
    const sessionId = "session-test-f7";
    const contract = { id: "C1", title: "契约", content: "必须" };
    const t1: TaskItem = {
      id: "T1",
      title: "根任务",
      domain: "backend",
      goal: "完成类型定义",
      depends_on: [],
      contracts: ["C1"],
      write_scope: ["src/types.ts"],
      deliverables: ["src/types.ts"],
      acceptance: { commands: ["npm test types"], criteria: ["类型通过"], confirmedCommands: ["npm test types"] },
      size: "S",
      difficulty: 1,
      assigned_model: "CLAUDE_CODE:sonnet",
    };
    const t2: TaskItem = {
      id: "T2",
      title: "中间任务",
      domain: "backend",
      goal: "实现业务",
      depends_on: ["T1"],
      contracts: ["C1"],
      write_scope: ["src/user.ts"],
      deliverables: ["src/user.ts"],
      acceptance: { commands: ["npm test user"], criteria: ["覆盖"], confirmedCommands: ["npm test user"] },
      size: "M",
      difficulty: 2,
      assigned_model: "CLAUDE_CODE:sonnet",
    };
    const t3: TaskItem = {
      id: "T3",
      title: "下游任务",
      domain: "testing",
      goal: "写测试",
      depends_on: ["T2"],
      contracts: ["C1"],
      write_scope: ["tests/user.test.ts"],
      deliverables: ["tests/user.test.ts"],
      acceptance: { commands: ["npm test"], criteria: ["绿"], confirmedCommands: ["npm test"] },
      size: "S",
      difficulty: 1,
      assigned_model: "CODEX_CLI:gpt-4o",
    };

    /** In-memory disk that simulates Rust cold-start rewrite on load. */
    function installDiskMock() {
      const disk = new Map<string, ipc.PipelineState>();
      const saveSpy = vi.spyOn(ipc, "savePipelineState").mockImplementation(async (_cwd, state) => {
        disk.set(state.sessionId, structuredClone(state));
      });
      const loadSpy = vi.spyOn(ipc, "loadPipelineState").mockImplementation(async (_cwd, id) => {
        const s = disk.get(id);
        if (!s) return null;
        const copy = structuredClone(s);
        let modified = false;
        if (copy.status === "RUNNING") {
          for (const t of copy.tasks) {
            if (t.state === "RUNNING" || t.state === "VERIFYING" || t.state === "REPAIRING") {
              t.state = "INTERRUPTED";
              modified = true;
            }
          }
          for (const [tid, st] of Object.entries(copy.taskStates)) {
            if (st === "RUNNING" || st === "VERIFYING" || st === "REPAIRING") {
              copy.taskStates[tid] = "INTERRUPTED";
              const logs = copy.taskLogs[tid] ?? [];
              logs.push("[系统] 检测到先前执行异常中断（应用关闭或崩溃），任务已自动置为 INTERRUPTED");
              copy.taskLogs[tid] = logs;
              modified = true;
            }
          }
          if (modified) copy.status = "INTERRUPTED";
        }
        return copy;
      });
      return { disk, saveSpy, loadSpy };
    }

    beforeEach(() => {
      resetSubAgentEventListener();
      useExecution.setState({ pipelines: {} });
      useProviders.setState({
        providers: mockProviders,
        scanning: false,
        pending: [],
        lastScan: null,
        error: null,
      });
      useSettings.setState({
        providerEnabled: { CLAUDE_CODE: true, CODEX_CLI: true },
        providerTrust: { CLAUDE_CODE: true, CODEX_CLI: true },
        isolation: "inplace",
        maxParallel: 3,
        agentIdleTimeoutMin: 30,
        agentHardTimeoutMin: 120,
      });
    });

    it("F7.a: round-trip restores full tasks/artifacts and does not auto-dispatch", async () => {
      const { saveSpy, loadSpy } = installDiskMock();
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);

      useExecution.getState().startPlanExecution(sessionId, [t1, t2, t3], "/test/repo", [contract]);
      // Legal transitions: T1 READY→RUNNING→VERIFYING→DONE; T2 PENDING→READY→RUNNING
      useExecution.getState().claimTaskRunning(sessionId, "T1", "turn-t1", "[test] T1 run");
      useExecution.getState().updateTaskState(sessionId, "T1", "VERIFYING", "[test] T1 verifying");
      useExecution.getState().updateTaskState(sessionId, "T1", "DONE", "[test] T1 done");
      useExecution.getState().recordTaskArtifact(sessionId, "T1", {
        taskId: "T1",
        title: t1.title,
        report: "types ready",
        changedFiles: ["src/types.ts"],
      });
      useExecution.getState().updateTaskState(sessionId, "T2", "READY", "[test] T2 ready");
      const claimed = useExecution.getState().claimTaskRunning(sessionId, "T2", "turn-t2", "[test] claim T2");
      expect(claimed).toBe(true);
      useExecution.getState().setTaskCwd(sessionId, "T2", "/test/repo/wt-t2");
      useExecution.getState().setTaskThreadId(sessionId, "T2", "cli-thread-t2");
      // Flush debounced cwd/thread writes via an immediate persist trigger
      useExecution.getState().pauseExecution(sessionId);
      useExecution.getState().resumeExecution(sessionId);

      expect(saveSpy.mock.calls.length).toBeGreaterThan(0);
      const lastSaved = saveSpy.mock.calls[saveSpy.mock.calls.length - 1][1] as ipc.PipelineState;
      expect(lastSaved.schemaVersion).toBe(2);
      expect(lastSaved.taskItems?.length).toBe(3);
      expect(lastSaved.taskStates["T2"]).toBe("RUNNING");

      useExecution.setState({ pipelines: {} });
      const recovered = await useExecution.getState().loadPipelineFromDisk("/test/repo", sessionId);
      expect(recovered).not.toBeNull();
      expect(recovered!.status).toBe("INTERRUPTED");
      expect(recovered!.taskStates["T1"]).toBe("DONE");
      expect(recovered!.taskStates["T2"]).toBe("INTERRUPTED");
      expect(recovered!.taskStates["T3"]).toBe("PENDING");

      const loadedT2 = recovered!.tasks.find((t) => t.id === "T2")!;
      expect(loadedT2.depends_on).toEqual(["T1"]);
      expect(loadedT2.goal).toBe("实现业务");
      expect(loadedT2.acceptance.commands).toContain("npm test user");
      expect(loadedT2.write_scope).toEqual(["src/user.ts"]);
      expect(recovered!.contracts?.[0].id).toBe("C1");
      expect(recovered!.upstreamArtifacts?.["T1"]?.changedFiles).toContain("src/types.ts");
      expect(recovered!.taskCwds?.["T2"]).toBe("/test/repo/wt-t2");
      expect(recovered!.threadIds?.["T2"]).toBe("cli-thread-t2");

      chatSpy.mockClear();
      const launched = await dispatchReadyTasks(sessionId);
      expect(launched).toHaveLength(0);
      expect(chatSpy).not.toHaveBeenCalled();
      expect(useExecution.getState().pipelines[sessionId].taskStates["T2"]).toBe("INTERRUPTED");
      expect(useExecution.getState().pipelines[sessionId].taskStates["T3"]).toBe("PENDING");

      saveSpy.mockRestore();
      loadSpy.mockRestore();
      chatSpy.mockRestore();
    });

    it("F7.d: continueTask resumes with thread id and task cwd; without thread re-dispatches", async () => {
      const chatSpy = vi.spyOn(ipc, "startChatTurn").mockResolvedValue(undefined);
      useExecution.getState().startPlanExecution(sessionId, [t1, t2], "/test/repo", [contract]);
      useExecution.getState().claimTaskRunning(sessionId, "T1", "turn-t1", "[test]");
      useExecution.getState().updateTaskState(sessionId, "T1", "VERIFYING");
      useExecution.getState().updateTaskState(sessionId, "T1", "DONE");
      // Mark T2 interrupted with thread + cwd
      useExecution.setState((s) => {
        const p = s.pipelines[sessionId];
        return {
          pipelines: {
            ...s.pipelines,
            [sessionId]: {
              ...p,
              status: "INTERRUPTED",
              taskStates: { ...p.taskStates, T2: "INTERRUPTED" },
              threadIds: { ...(p.threadIds ?? {}), T2: "thr-continue" },
              taskCwds: { ...(p.taskCwds ?? {}), T2: "/test/repo/wt-t2" },
            },
          },
        };
      });

      await continueTask(sessionId, "T2");
      expect(chatSpy).toHaveBeenCalledTimes(1);
      const req = chatSpy.mock.calls[0][0];
      expect(req.resume).toBe("thr-continue");
      expect(req.cwd).toBe("/test/repo/wt-t2");
      expect(req.prompt).toContain("继续未完成的任务");
      expect(useExecution.getState().pipelines[sessionId].taskStates["T2"]).toBe("RUNNING");
      expect(useExecution.getState().pipelines[sessionId].status).toBe("RUNNING");

      // Without thread id → fresh dispatch using task cwd
      chatSpy.mockClear();
      useExecution.setState((s) => {
        const p = s.pipelines[sessionId];
        return {
          pipelines: {
            ...s.pipelines,
            [sessionId]: {
              ...p,
              status: "INTERRUPTED",
              taskStates: { ...p.taskStates, T2: "INTERRUPTED" },
              threadIds: {},
              taskTurnIds: {},
              taskCwds: { ...(p.taskCwds ?? {}), T2: "/test/repo/wt-t2" },
            },
          },
        };
      });
      resetSubAgentEventListener();
      await continueTask(sessionId, "T2");
      expect(chatSpy).toHaveBeenCalledTimes(1);
      expect(chatSpy.mock.calls[0][0].resume).toBeNull();
      expect(chatSpy.mock.calls[0][0].cwd).toBe("/test/repo/wt-t2");

      chatSpy.mockRestore();
    });
  });
});
