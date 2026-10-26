import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import { ContextSuggestionBar } from "./ContextSuggestionBar";
import type { ContextActionPlan } from "../../lib/ipc";
vi.mock("react-i18next",()=>({useTranslation:()=>({t:(key:string)=>key})}));
it("does not expose Apply for a blocked plan or one still in cooldown",()=>{
 const plan:ContextActionPlan={recommendedAction:"COMPACT",trigger:"HARD_LIMIT",continueCostUsd:1,compactCostUsd:.5,freshCostUsd:.6,estimatedSavingPercentage:.5,warmNow:false,cacheTtlRemainingSeconds:0,continueDisqualified:true,explanation:"blocked",cooldownBlocked:true,cooldownRemainingTurns:1};
 for(const override of [{},{cooldownBlocked:false}]) {
  const html=renderToStaticMarkup(<ContextSuggestionBar plan={{...plan,...override}} onApply={()=>{throw Error("must not apply");}} onDismiss={()=>{}}/>);
  expect(html).toContain("workspace.context.cooldownBlocked");expect(html).not.toContain("<button");
 }
});
