import { describe, expect, it } from "vitest";
import { invalidatedTasks } from "./replan";
import type { Plan, TaskItem } from "./ipc";
const task = (id: string, depends_on: string[] = []): TaskItem => ({ id, title: id, goal: id, domain: "backend", depends_on, contracts: [], write_scope: [id], deliverables: [], acceptance: { commands: ["check"], criteria: [] }, size: "S", difficulty: 1 });
const plan: Plan = { version: 1, tasks: [task("A"), task("B", ["A"]), task("C", ["B"]), task("D")], contracts: [], spec: { raw: "", goal: "goal", inScope: [], outOfScope: [], decisions: [] }, rawMarkdown: "" };
describe("m2c replan input identity", () => {
  it("invalidates the entire downstream chain while retaining independent work", () => {
    const next = structuredClone(plan); next.tasks[0].goal = "new behavior";
    expect([...invalidatedTasks(plan, next)].sort()).toEqual(["A", "B", "C"]);
  });
  it("invalidates contract consumers even when contract IDs are unchanged", () => {
    const old = structuredClone(plan); old.contracts = [{ id: "API", title: "API", content: "old" }]; old.tasks[0].contracts = ["API"];
    const next = structuredClone(old); next.contracts[0].content = "new";
    expect([...invalidatedTasks(old, next)].sort()).toEqual(["A", "B", "C"]);
  });
  it("invalidates changed execution models and new global instructions", () => {
    const next = structuredClone(plan); next.tasks[0].assigned_model = "OPENCODE:model";
    expect([...invalidatedTasks(plan, next)].sort()).toEqual(["A", "B", "C"]);
    next.notes = "must preserve compatibility";
    expect(invalidatedTasks(plan, next).size).toBe(4);
  });
  it("invalidates consumers of removed inputs and never reuses changed gate definitions", () => {
    const next = structuredClone(plan); next.tasks = next.tasks.slice(1);
    expect([...invalidatedTasks(plan, next)].sort()).toEqual(["A", "B", "C"]);
    const gates = structuredClone(plan); gates.tasks[3].acceptance.commands = ["new-check"];
    expect([...invalidatedTasks(plan, gates)]).toEqual(["D"]);
  });
});

describe("m2c identities across persistence", () => {
  it("ignores key order and default optional values without ignoring changed requirements", async () => {
    const { taskIdentity, planIdentity } = await import("./planIdentity");
    const original = task("A");
    const reordered = Object.fromEntries(Object.entries(original).reverse()) as unknown as TaskItem;
    expect(taskIdentity(reordered)).toBe(taskIdentity({ ...original, assigned_model: null, model_params: null, acceptance: { ...original.acceptance, confirmedCommands: [] } }));
    const changed = structuredClone(plan); changed.spec.goal = "new behavior";
    expect(planIdentity(changed)).not.toBe(planIdentity(plan));
    expect(taskIdentity({ ...original, model_params: { fast: false } })).toBe(taskIdentity(original));
    expect(taskIdentity({ ...original, model_params: { fast: true } })).not.toBe(taskIdentity(original));
  });
});
