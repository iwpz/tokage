import { describe, it, expect, vi, beforeEach } from "vitest";
import { resolveTaskProviderAndModel, buildSubAgentPrompt } from "./taskRunner";
import type { ProviderInfo, TaskItem } from "./ipc";

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
});
