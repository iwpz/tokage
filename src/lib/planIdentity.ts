import type { Plan, TaskItem } from "./ipc";

/** Ignore object key order and optional nulls across the JS/Rust persistence boundary. */
export function canonicalIdentity(value: unknown): string {
  const normalize = (v: unknown): unknown => {
    if (Array.isArray(v)) return v.map(normalize);
    if (v && typeof v === "object") return Object.fromEntries(Object.entries(v)
      .filter(([, item]) => item != null).sort(([a], [b]) => a.localeCompare(b))
      .map(([key, item]) => [key, normalize(item)]));
    return v;
  };
  return JSON.stringify(normalize(value));
}
export function taskIdentity(task: TaskItem): string {
  const params = task.model_params;
  return canonicalIdentity({ ...task,
    model_params: params?.effort || params?.fast || params?.tier ? { ...params, fast: params.fast || undefined } : undefined,
    acceptance: { ...task.acceptance, confirmedCommands: task.acceptance.confirmedCommands ?? [] },
  });
}
export function planIdentity(plan: Plan): string {
  return canonicalIdentity({ version: plan.version, spec: plan.spec, contracts: plan.contracts,
    tasks: plan.tasks.map(taskIdentity), notes: plan.notes });
}
