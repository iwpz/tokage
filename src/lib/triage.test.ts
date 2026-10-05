import { describe, expect, it } from "vitest";
import { triage } from "./models";

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
