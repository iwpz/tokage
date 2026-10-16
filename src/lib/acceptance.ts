import { executeGateCommand, type PipelineGateResult, type ScopeSnapshot } from "./ipc";

/** Compare content, index and HEAD without depending on JSON object insertion order. */
export function sameAcceptanceContent(a: ScopeSnapshot, b: ScopeSnapshot): boolean {
  return a.root === b.root && a.workdir === b.workdir && a.head === b.head && a.clean === b.clean
    && JSON.stringify(Object.entries(a.files).sort()) === JSON.stringify(Object.entries(b.files).sort());
}

/** Fail fast, but retain an explicit outcome for every requested command. */
export async function runAcceptanceCommands(
  commands: string[], taskId: string,
  execute: (command: string) => ReturnType<typeof executeGateCommand>,
  active: () => boolean,
  record: (result: PipelineGateResult) => void,
): Promise<{ passed: boolean; aborted: boolean; results: PipelineGateResult[] }> {
  const results: PipelineGateResult[] = [];
  let stopped = false;
  let aborted = false;
  for (const command of commands) {
    let result: PipelineGateResult;
    if (!active()) { stopped = true; aborted = true; }
    if (stopped) {
      result = { taskId, command, passed: false, status: aborted ? "cancelled" : "not_run", durationMs: 0, outputTail: aborted ? "Acceptance cancelled" : "Not executed after earlier failure" };
    } else {
      try {
        const value = await execute(command);
        aborted = !active();
        result = { taskId, ...value, command, outputTail: value.outputTail || value.errorMessage || "", passed: !aborted && value.passed,
          status: aborted ? "cancelled" : value.passed ? "passed" : "failed" };
      } catch (error) {
        aborted = !active();
        result = { taskId, command, passed: false, status: aborted ? "cancelled" : "error", durationMs: 0, outputTail: String(error) };
      }
      stopped = !result.passed;
    }
    results.push(result);
    record(result);
  }
  return { results, aborted, passed: commands.length > 0 && results.every(r => r.passed) && !aborted };
}
