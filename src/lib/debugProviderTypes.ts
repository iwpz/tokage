/** Execution routing is independent of the model selected by matching/the picker. */
export type DebugOutput = "off" | "mock" | "luna" | "deepseek";
export type MockScenario = "success" | "repair" | "error" | "slow";

export interface DebugProviderConfig {
  output: DebugOutput;
  scenario: MockScenario;
  chunkDelayMs: number;
  /** Real CLI used by the paid surrogate; never inferred from the requested model. */
  lunaProvider: string;
  lunaModel: string;
  deepseekProvider: string;
  deepseekModel: string;
  /** Optional direct-chat response; plans and task reports keep their structured format. */
  mockReply: string;
}

export const DEFAULT_DEBUG_PROVIDER: DebugProviderConfig = {
  output: "off",
  scenario: "success",
  chunkDelayMs: 30,
  lunaProvider: "CODEX_CLI",
  lunaModel: "gpt-6-luna",
  deepseekProvider: "AIDER",
  deepseekModel: "deepseek/deepseek-v4-pro",
  mockReply: "",
};

export interface DebugRoute {
  output: Exclude<DebugOutput, "off">;
  requestedProvider: string;
  requestedModel: string;
  actualProvider: string;
  actualModel: string;
  workdir?: string;
}

export function debugTarget(config: DebugProviderConfig): { provider: string; model: string } | null {
  if (config.output === "luna") return { provider: config.lunaProvider, model: config.lunaModel.trim() };
  if (config.output === "deepseek") return { provider: config.deepseekProvider, model: config.deepseekModel.trim() };
  return null;
}

