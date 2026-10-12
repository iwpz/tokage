import { describe, expect, it } from "vitest";
import { isModelBlocked, pickCheapest, selectableGroups, triage } from "./models";

describe("triage heuristic (frontend)", () => {
  it("routes multi-domain requirements to plan", () => {
    expect(
      triage("开发用户设置页面组件，同时提供后端的更新接口和数据库迁移脚本。")
    ).toBe("plan");
    expect(
      triage("Build real-time notification React UI components and WebSocket server API with Redis")
    ).toBe("plan");
  });

  it("routes feature implementation and scaffolding to plan", () => {
    expect(triage("从零搭建一个基于 Rust 的分布式任务队列系统")).toBe("plan");
    expect(
      triage("Implement an end-to-end OAuth2 login flow with refresh tokens")
    ).toBe("plan");
    expect(triage("重构整个状态管理模块并解耦业务副作用")).toBe("plan");
  });

  it("routes multi-file mentions to plan", () => {
    expect(
      triage("请同步更新 a.rs, b.rs, c.rs, d.rs 以及 e.rs 中的版本号与导出宏")
    ).toBe("plan");
  });

  it("routes single issue questions and bug fixes to direct", () => {
    expect(
      triage("这个报错怎么解决？Caused by: std::io::Error: NotFound")
    ).toBe("direct");
    expect(
      triage("Fix this error: TypeError: Cannot read properties of undefined")
    ).toBe("direct");
    expect(
      triage("解释一下这段代码里的 tokio::select! 宏是怎么工作的")
    ).toBe("direct");
    expect(triage("改一下这个函数名字为 getUserById")).toBe("direct");
  });

  it("routes pinpoint locations to direct", () => {
    expect(triage("src/auth/jwt.rs:42 这里数组越界了，帮忙改一下")).toBe("direct");
    expect(triage("第 88 行的 if 语句判断条件写反了，修一下")).toBe("direct");
  });

  it("defaults ambiguous requests to plan", () => {
    expect(triage("")).toBe("plan");
    expect(triage("探讨一下未来演进方向与技术选型可能")).toBe("plan");
  });
});

describe("isModelBlocked and selectableGroups model filtering", () => {
  it("accurately detects blocked models by name or keyword", () => {
    expect(isModelBlocked("claude-fable-5-1", ["fable 5.1", "fable-5-1"])).toBe(true);
    expect(isModelBlocked("fable", ["fable"])).toBe(true);
    expect(isModelBlocked("claude-opus-5-5", ["fable"])).toBe(false);
    expect(isModelBlocked("gpt-6-luna", [])).toBe(false);
  });

  it("handles tiered banning levels correctly (moderate blocks fable/astra, strict additionally blocks opus/sol)", () => {
    // moderate blocks fable and astra, but allows opus and sol
    expect(isModelBlocked("claude-fable-5-1", [], undefined, "moderate")).toBe(true);
    expect(isModelBlocked("gpt-6-astra", [], undefined, "moderate")).toBe(true);
    expect(isModelBlocked("claude-opus-5-5", [], undefined, "moderate")).toBe(false);
    expect(isModelBlocked("gpt-6.1-sol", [], undefined, "moderate")).toBe(false);
    expect(isModelBlocked("gpt-6-sol", [], undefined, "moderate")).toBe(false);

    // strict blocks fable, astra, and additionally opus and sol
    expect(isModelBlocked("claude-fable-5-1", [], undefined, "strict")).toBe(true);
    expect(isModelBlocked("gpt-6-astra", [], undefined, "strict")).toBe(true);
    expect(isModelBlocked("claude-opus-5-5", [], undefined, "strict")).toBe(true);
    expect(isModelBlocked("gpt-6.1-sol", [], undefined, "strict")).toBe(true);
    expect(isModelBlocked("gpt-6-sol", [], undefined, "strict")).toBe(true);

    // off blocks none of them by level
    expect(isModelBlocked("claude-fable-5-1", [], undefined, "off")).toBe(false);
    expect(isModelBlocked("claude-opus-5-5", [], undefined, "off")).toBe(false);
  });

  it("filters blocked models out of selectable groups completely", () => {
    const mockProviders = [
      {
        id: "CLAUDE_CODE",
        name: "Claude Code",
        executable: "claude",
        featured: true,
        status: "READY",
        version: "2.1.288",
        path: "/usr/local/bin/claude",
        models: [
          { id: "fable", name: "Fable" },
          { id: "sonnet", name: "Sonnet" },
        ],
        modelGroups: [
          {
            key: "fable",
            name: "Fable",
            efforts: [],
            contexts: [],
            thinking: false,
            fast: false,
            tags: [],
            tiers: [],
            variants: [{ id: "fable", thinking: false, fast: false }],
          },
          {
            key: "sonnet",
            name: "Sonnet",
            efforts: [],
            contexts: [],
            thinking: false,
            fast: false,
            tags: [],
            tiers: [],
            variants: [{ id: "sonnet", thinking: false, fast: false }],
          },
        ],
        trustArgs: [],
        installMethods: [],
        guidance: [],
        chat: true,
      },
    ] as any;

    const enabled = { CLAUDE_CODE: true };
    const trust = { CLAUDE_CODE: true };

    const unblocked = selectableGroups(mockProviders, enabled, trust, "plan", []);
    expect(unblocked[0].models.map((m) => m.key)).toEqual(["fable", "sonnet"]);

    const blocked = selectableGroups(mockProviders, enabled, trust, "plan", ["fable"]);
    expect(blocked[0].models.map((m) => m.key)).toEqual(["sonnet"]);
  });

  it("pickCheapest skips blocked models and prefers lower cost", () => {
    const mockProviders = [
      {
        id: "CLAUDE_CODE",
        name: "Claude Code",
        executable: "claude",
        featured: true,
        status: "READY",
        version: "2.1.288",
        path: "/usr/local/bin/claude",
        models: [
          { id: "fable", name: "Fable" },
          { id: "sonnet", name: "Sonnet" },
          { id: "haiku", name: "Haiku" },
        ],
        modelGroups: [
          {
            key: "fable",
            name: "Fable",
            efforts: [],
            contexts: [],
            thinking: false,
            fast: false,
            tags: [],
            tiers: [],
            variants: [{ id: "fable", thinking: false, fast: false }],
          },
          {
            key: "sonnet",
            name: "Sonnet",
            efforts: [],
            contexts: [],
            thinking: false,
            fast: false,
            tags: [],
            tiers: [],
            variants: [{ id: "sonnet", thinking: false, fast: false }],
          },
          {
            key: "haiku",
            name: "Haiku",
            efforts: [],
            contexts: [],
            thinking: false,
            fast: false,
            tags: [],
            tiers: [],
            variants: [{ id: "haiku", thinking: false, fast: false }],
          },
        ],
        trustArgs: [],
        installMethods: [],
        guidance: [],
        chat: true,
      },
    ] as any;

    const enabled = { CLAUDE_CODE: true };
    const trust = { CLAUDE_CODE: true };
    const groups = selectableGroups(mockProviders, enabled, trust, "plan", ["fable"]);
    const pick = pickCheapest(groups);
    expect(pick?.ref).toEqual({ provider: "CLAUDE_CODE", model: "haiku" });
  });
});
