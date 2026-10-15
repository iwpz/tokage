import { demoScript } from "./demoScript";
import type { GateCommandResult, Plan } from "./ipc";
import type { DebugProviderConfig } from "./debugProviderTypes";
import markdown from "../../src-tauri/resources/debug/plan.md?raw";

/** Browser preview uses the same scenario as the Rust fixture parser. */
export function debugMockPlan(): Plan {
  return {
    spec: { raw: markdown.split("# Contracts")[0].replace("# Spec\n", ""), goal: "Exercise the complete Tokage pipeline with the Debug Provider.", inScope: ["scripted implementation, regression test, reports and delivery"], outOfScope: ["real model calls and changes to the original project"], decisions: ["All model output and acceptance feedback are simulated."] },
    contracts: [{ id: "C1", title: "Debug result", content: "```ts\nexport const debugResult: string;\n```" }],
    tasks: [
      { id: "T1", domain: "backend-api", title: "Produce the debug result", goal: "Export a deterministic debug result", depends_on: [], contracts: ["C1"], write_scope: ["src/debug-result.ts"], deliverables: ["debug result"], acceptance: { commands: ["npm test"], criteria: ["Deterministic result is available"] }, size: "S", difficulty: 1 },
      { id: "T2", domain: "testing", title: "Verify the debug result", goal: "Verify the upstream result and report acceptance", depends_on: ["T1"], contracts: ["C1"], write_scope: ["tests/debug-result.test.ts"], deliverables: ["regression test report"], acceptance: { commands: ["npm test"], criteria: ["Upstream result matches the contract"] }, size: "S", difficulty: 1 },
    ],
    notes: "This is a simulated run. The original project is untouched.", rawMarkdown: markdown,
  };
}

const gates = new Map<string, number>();
export function debugMockGate(command: string, cwd: string, runId: string | undefined, config: DebugProviderConfig): GateCommandResult {
  const key = `${cwd}:${runId ?? "delivery"}`;
  const count = gates.get(key) ?? 0;
  gates.set(key, count + 1);
  const passed = config.demo
    ? config.scenario !== "repair" || !runId?.endsWith(":T2") || count > 0
    : config.scenario !== "error" && (config.scenario !== "repair" || runId?.startsWith("gate:delivery:") || count > 0);
  return { command, passed: !!passed, exitCode: passed ? 0 : 1, durationMs: 0, outputTail: config.demo ? demoScript(config.demoLanguage ?? "en")[passed ? "gatePass" : "gateFail"] : passed ? "[Debug Provider] Simulated acceptance passed; no command was executed." : "[Debug Provider] Simulated acceptance failed. Repair the task and retry." };
}
