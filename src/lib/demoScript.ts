import fixture from "../../src-tauri/resources/demo/pocket-tasks.json";
import type { Plan } from "./ipc";
import type { DemoLanguage } from "./demoIdentity";

export const demoFixture = fixture;
export function demoScript(language: DemoLanguage) { return fixture.locales[language]; }
export function demoPlan(language: DemoLanguage): Plan {
  return structuredClone(demoScript(language).plan) as Plan;
}
export function demoFile(path: string, language: DemoLanguage, repair: boolean, scenario: string): string | undefined {
  if (path === "README.md") return demoScript(language).readme;
  const content = fixture.files[path as keyof typeof fixture.files];
  if (path === "src/task-service.js" && scenario === "repair" && !repair) {
    return content.replace("export function remainingCount(tasks) { return tasks.filter(t => !t.completed).length; }", "export function remainingCount(tasks) { return tasks.length; }");
  }
  return content;
}
export function demoReport(language: DemoLanguage, taskCode?: string | null): string {
  const script = demoScript(language);
  const task = script.plan.tasks.find(t => t.id === taskCode);
  return `${script.working}\n\n### What was completed\n${task?.title ?? "Pocket Tasks"}\n${script.completed}\n### Deviations from contracts\n${script.none}\n### Hints for downstream\n${script.downstream}\n### Self-check against acceptance criteria\n- ${script.acceptance}\n### Defects found\n${script.none}\n`;
}
