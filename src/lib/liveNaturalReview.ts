import type { Plan } from "./ipc";

/** Acceptance-only review. Reject unsuitable generated plans; never rewrite them. */
export function reviewNaturalPlan(plan: Plan): void {
  const files = new Set(["pricing.mjs", "format.mjs", "invoice.mjs"]);
  const commands = new Set(["npm run test:pricing", "npm run test:format", "npm run test:integration", "npm test", "npm run test"]);
  const roots = plan.tasks.filter((t) => !t.depends_on.length);
  if (roots.length < 2 || !plan.tasks.some((t) => t.depends_on.length >= 2) || !plan.contracts.length)
    throw new Error("Generated plan lacks parallel work, integration join or public contracts");
  const ids = new Set(plan.tasks.map((t) => t.id));
  if (ids.size !== plan.tasks.length || plan.tasks.some((t) => t.depends_on.some((d) => !ids.has(d))))
    throw new Error("Generated plan has duplicate or missing task identities");
  for (const task of plan.tasks) {
    if (!task.write_scope.length || task.write_scope.some((p) => !files.has(p)))
      throw new Error(`Unsafe generated write scope: ${task.id}`);
    if (!task.acceptance.commands.length || task.acceptance.commands.some((c) => !commands.has(c)))
      throw new Error(`Unreviewed generated acceptance command: ${task.id}`);
  }
  if (!plan.tasks.some((t) => t.acceptance.commands.some((c) => c === "npm test" || c === "npm run test")))
    throw new Error("Generated plan omits full independent acceptance");
  for (let i = 0; i < roots.length; i++) for (let j = i + 1; j < roots.length; j++)
    if (roots[i].write_scope.some((p) => roots[j].write_scope.includes(p)))
      throw new Error("Parallel tasks have overlapping write scope");
}
