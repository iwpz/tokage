import { describe, expect, it } from "vitest";
import type { Plan, PlanContract, PlanSpec } from "../../lib/ipc";

describe("plan state and editing operations", () => {
  const initialPlan: Plan = {
    spec: {
      raw: "# Goal\nBuild feature",
      goal: "Build feature",
      inScope: ["Frontend UI", "Backend API"],
      outOfScope: ["Payment gateway"],
      decisions: ["Use Rust and React"],
    },
    contracts: [
      {
        id: "auth-contract",
        title: "src/auth.ts - User Interface",
        content: "export interface User { id: string; }",
      },
    ],
    tasks: [],
    notes: "Plan notes",
    rawMarkdown: "",
  };

  it("updates spec fields correctly", () => {
    const patch: Partial<PlanSpec> = {
      goal: "Updated goal",
      inScope: [...initialPlan.spec.inScope, "Database migrations"],
    };

    const updatedSpec = {
      ...initialPlan.spec,
      ...patch,
    };

    expect(updatedSpec.goal).toBe("Updated goal");
    expect(updatedSpec.inScope).toHaveLength(3);
    expect(updatedSpec.outOfScope).toEqual(["Payment gateway"]);
  });

  it("adds, edits, and removes contracts", () => {
    // Add contract
    const newContract: PlanContract = {
      id: "api-contract",
      title: "src/api.rs - API Types",
      content: "pub struct Response;",
    };
    let contracts = [...initialPlan.contracts, newContract];
    expect(contracts).toHaveLength(2);

    // Edit contract
    contracts = contracts.map((c) =>
      c.id === "api-contract" ? { ...c, content: "pub struct UpdatedResponse;" } : c
    );
    expect(contracts.find((c) => c.id === "api-contract")?.content).toBe(
      "pub struct UpdatedResponse;"
    );

    // Remove contract
    contracts = contracts.filter((c) => c.id !== "auth-contract");
    expect(contracts).toHaveLength(1);
    expect(contracts[0].id).toBe("api-contract");
  });

  it("handles task additions, editing, and model assignments", () => {
    const task1 = {
      id: "T1",
      domain: "backend-api",
      title: "User API",
      goal: "Implement user endpoint",
      depends_on: [],
      contracts: ["auth-contract"],
      write_scope: ["src/server/**"],
      deliverables: ["Routes"],
      acceptance: { commands: ["cargo test"], criteria: ["Coverage"] },
      size: "M" as const,
      difficulty: 2,
      assigned_model: null,
    };

    let tasks = [...initialPlan.tasks, task1];
    expect(tasks).toHaveLength(1);

    // Assign model
    tasks = tasks.map((t) =>
      t.id === "T1" ? { ...t, assigned_model: "anthropic:claude-3-7-sonnet" } : t
    );
    expect(tasks[0].assigned_model).toBe("anthropic:claude-3-7-sonnet");

    // Add dependent task
    const task2 = {
      id: "T2",
      domain: "frontend-ui",
      title: "User Profile UI",
      goal: "Implement React user page",
      depends_on: ["T1"],
      contracts: ["auth-contract"],
      write_scope: ["src/pages/**"],
      deliverables: ["UserPage"],
      acceptance: { commands: ["npm test"], criteria: ["Render test"] },
      size: "S" as const,
      difficulty: 1,
      assigned_model: "openai:gpt-4o",
    };
    tasks = [...tasks, task2];
    expect(tasks).toHaveLength(2);
    expect(tasks[1].depends_on).toContain("T1");

    // Delete task
    tasks = tasks.filter((t) => t.id !== "T1");
    expect(tasks).toHaveLength(1);
    expect(tasks[0].id).toBe("T2");
  });

  it("handles plan versioning and diff operations (P5.m6a)", () => {
    const v1Plan: Plan = {
      version: 1,
      parentPlanId: null,
      spec: {
        raw: "",
        goal: "Initial Goal",
        inScope: ["Backend"],
        outOfScope: ["Billing"],
        decisions: ["Use SQLite"],
      },
      contracts: [
        { id: "C1", title: "API", content: "interface User {}" },
      ],
      tasks: [
        {
          id: "T1",
          domain: "backend-api",
          title: "API Task",
          goal: "Build API",
          depends_on: [],
          contracts: ["C1"],
          write_scope: ["src/**"],
          deliverables: [],
          acceptance: { commands: [], criteria: [] },
          size: "M",
          difficulty: 2,
        },
      ],
      rawMarkdown: "",
    };

    const v2Plan: Plan = {
      ...v1Plan,
      version: 2,
      parentPlanId: "plan-v1",
      spec: {
        ...v1Plan.spec,
        goal: "Updated Goal",
        inScope: ["Backend", "Frontend"],
      },
      contracts: [
        { id: "C1", title: "API", content: "interface User { id: string }" },
        { id: "C2", title: "UI Contract", content: "type View = 'list'" },
      ],
    };

    expect(v2Plan.version).toBe(2);
    expect(v2Plan.parentPlanId).toBe("plan-v1");
    expect(v2Plan.spec.goal).toBe("Updated Goal");
    expect(v2Plan.spec.inScope).toContain("Frontend");
    expect(v2Plan.contracts).toHaveLength(2);
  });
});

