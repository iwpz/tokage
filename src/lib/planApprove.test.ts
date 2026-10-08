import { describe, it, expect, vi, beforeEach } from "vitest";
import * as ipc from "./ipc";
import type { Plan, TaskItem } from "./ipc";
import { useExecution } from "../stores/execution";

/**
 * F1.e: after validatePlan repairs the plan (e.g. serializes write_scope conflicts),
 * execution must start from that repaired plan — not the pre-validate closure value.
 */
describe("F1.e validated plan drives execution", () => {
  beforeEach(() => {
    useExecution.setState({ pipelines: {} });
  });

  it("startPlanExecution receives depends_on edges added by validatePlan", async () => {
    const t1: TaskItem = {
      id: "T1",
      title: "A",
      domain: "backend",
      goal: "a",
      depends_on: [],
      contracts: [],
      write_scope: ["src/**"],
      deliverables: [],
      acceptance: { commands: [], criteria: [] },
      size: "S",
      difficulty: 1,
    };
    const t2Raw: TaskItem = {
      id: "T2",
      title: "B",
      domain: "backend",
      goal: "b",
      depends_on: [],
      contracts: [],
      write_scope: ["src/server/**"],
      deliverables: [],
      acceptance: { commands: [], criteria: [] },
      size: "S",
      difficulty: 1,
    };
    const t2Fixed: TaskItem = { ...t2Raw, depends_on: ["T1"] };

    const rawPlan: Plan = {
      spec: { raw: "", goal: "", inScope: [], outOfScope: [], decisions: [] },
      contracts: [],
      tasks: [t1, t2Raw],
      notes: "",
      rawMarkdown: "",
    };
    const validatedPlan: Plan = { ...rawPlan, tasks: [t1, t2Fixed] };

    vi.spyOn(ipc, "validatePlan").mockResolvedValue({
      isValid: true,
      plan: validatedPlan,
      issues: [
        {
          severity: "warning",
          rule: "WRITE_SCOPE_OVERLAP",
          message: "serialized",
          taskId: "T2",
        },
      ],
    });
    vi.spyOn(ipc, "persistPlan").mockResolvedValue({
      planId: "sess-f1e",
      planDir: "/repo/.tokage/plans/sess-f1e",
      filesWritten: ["tasks.yaml"],
    });

    // Same sequence as PlanReview.handleApprove after F1.e: save → use returned plan
    const val = await ipc.validatePlan({ plan: rawPlan });
    await ipc.persistPlan("/repo", "sess-f1e", val.plan);
    const saved = val.plan;

    useExecution.getState().startPlanExecution(
      "sess-f1e",
      saved.tasks,
      "/repo",
      saved.contracts ?? [],
    );

    const pipeline = useExecution.getState().pipelines["sess-f1e"];
    expect(pipeline.tasks.find((t) => t.id === "T2")?.depends_on).toEqual(["T1"]);
    // Stale closure plan would have left T2 with no depends_on:
    expect(rawPlan.tasks.find((t) => t.id === "T2")?.depends_on).toEqual([]);
  });
});
