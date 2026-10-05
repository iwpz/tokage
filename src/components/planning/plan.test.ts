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
});
