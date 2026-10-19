import { describe, expect, it } from "vitest";
import type { Plan } from "./ipc";
import { reviewNaturalPlan } from "./liveNaturalReview";
const fixture = () => ({ contracts: [{ id: "invoice-api" }], tasks: [
  { id: "P", depends_on: [], write_scope: ["pricing.mjs"], acceptance: { commands: ["npm run test:pricing"] } },
  { id: "F", depends_on: [], write_scope: ["format.mjs"], acceptance: { commands: ["npm run test:format"] } },
  { id: "I", depends_on: ["P", "F"], write_scope: ["invoice.mjs"], acceptance: { commands: ["npm run test:integration", "npm test"] } },
] } as Plan);
describe("natural-language generated plan review", () => {
  it("accepts a parallel graph without modifying the generated plan", () => {
    const p = fixture(), before = structuredClone(p);
    reviewNaturalPlan(p); expect(p).toEqual(before);
  });
  it("accepts only the equivalent npm run test alias without changing the plan", () => {
    const p = fixture(); p.tasks[2].acceptance.commands[1] = "npm run test";
    const before = structuredClone(p); reviewNaturalPlan(p); expect(p).toEqual(before);
    p.tasks[2].acceptance.commands[1] = "npm run test -- --skip-all";
    expect(() => reviewNaturalPlan(p)).toThrow();
  });
  it("rejects test tampering, arbitrary shell commands and missing independent acceptance", () => {
    for (const mutate of [
      (p: Plan) => { p.tasks[0].write_scope = ["check.mjs"]; },
      (p: Plan) => { p.tasks[0].acceptance.commands = ["true"]; },
      (p: Plan) => { p.tasks[2].acceptance.commands = ["npm run test:integration"]; },
      (p: Plan) => { p.contracts = []; },
    ]) { const p = fixture(); mutate(p); expect(() => reviewNaturalPlan(p)).toThrow(); }
  });
  it("rejects missing dependencies and overlapping parallel write scope", () => {
    const missing = fixture(); missing.tasks[2].depends_on = ["missing", "F"];
    expect(() => reviewNaturalPlan(missing)).toThrow();
    const overlap = fixture(); overlap.tasks[1].write_scope = ["pricing.mjs"];
    expect(() => reviewNaturalPlan(overlap)).toThrow();
  });
});
