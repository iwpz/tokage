import { DEFAULT_DEBUG_PROVIDER, type DebugProviderConfig, type MockScenario } from "./debugProviderTypes";

export const DEMO_PROJECT_ID = "tokage-demo-project";
export const DEMO_SESSION_PREFIX = "tokage-demo-";
export type DemoLanguage = "en" | "zh";

/** Identity survives DB hydration; it never depends on the currently open project. */
export function demoIdentity(sessionId?: string | null): { scenario: MockScenario; language: DemoLanguage } | null {
  const match = /^tokage-demo-(success|repair|error|slow)-(en|zh)-/.exec(sessionId ?? "");
  return match ? { scenario: match[1] as MockScenario, language: match[2] as DemoLanguage } : null;
}

export function demoConfig(sessionId?: string | null, delay = 30): DebugProviderConfig | null {
  const identity = demoIdentity(sessionId);
  return identity ? { ...DEFAULT_DEBUG_PROVIDER, output: "mock", scenario: identity.scenario, chunkDelayMs: delay, demo: true, demoLanguage: identity.language } : null;
}
