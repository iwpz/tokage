import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import type { ResolvedContextInputs } from "../../lib/contextInputs";
import { ContextDecisionDetails } from "./ContextDecisionDetails";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string, values?: object) => `${key} ${values ? JSON.stringify(values) : ""}` }) }));
const inputs = {
  params: { remainingTurns: 1, apiCallsPerTurn: 1, growthPerTurn: 4000, minSaving: .15, cacheReadMultiplier: .02, cacheWriteMultiplier: 1, pIn: .15, pOut: .6 },
  preview: { recommendedAction: "CONTINUE", explanation: "Continue costs less", continueCostUsd: .006529, compactCostUsd: .011973, freshCostUsd: .011307, cooldownRemainingTurns: 0 },
  cooldown: { known: true },
} as ResolvedContextInputs;

it("exposes the decision and small estimated costs without offering an execution action", () => {
  const html = renderToStaticMarkup(<ContextDecisionDetails inputs={inputs}/>);
  expect(html).toContain("Continue costs less");
  expect(html).toContain("0.006529");
  expect(html).toContain("decisionForecast");
  expect(html).toContain("decisionEstimate");
  expect(html).toContain("decisionActions.CONTINUE");
  expect(html).not.toContain("<button");
});
it("keeps unknown history and unavailable retention visible even when a planner recommends an action", () => {
  const html = renderToStaticMarkup(<ContextDecisionDetails inputs={{...inputs, cooldown: {...inputs.cooldown, known:false}, preview:{...inputs.preview, recommendedAction:"COMPACT"}, compactUnavailableReason:"Missing current context"}}/>);
  expect(html).toContain("decisionBlocked");
  expect(html).toContain("cooldownUnknown");
  expect(html).toContain("Missing current context");
  expect(html).not.toContain("decisionActions.COMPACT");
  expect(html).not.toContain("<button");
});
it("does not show stale costs when inputs are unavailable", () => {
  expect(renderToStaticMarkup(<ContextDecisionDetails inputs={null}/>)).toBe("");
});
