import {it,expect} from "vitest";
import {renderToString} from "react-dom/server";
import "../../i18n";
import {AccountingCostCard,CounterfactualCard,AccountingEvidence} from "./AccountingView";
import type {DashboardAccounting} from "../../lib/ipc";
it("renders unknown totals separately from known zero and keeps negative comparison evidence",()=>{
 const data={totals:{reportedUsd:null,knownReportedUsd:0,unknownUsdReceipts:1,estimatedUsd:2},phases:{},comparableCount:1,comparisons:[{id:1,kind:"CONTEXT",status:"COUNTERFACTUAL_NOT_MEASURED_SAVING",baselineUsd:1,actualUsd:2,deltaUsd:-1,detailJson:'{"basis":"first_turn_counterfactual_v1"}'}],receipts:[{key:"turn:one",provider:"p",model:"m",phase:"REVIEW",status:"Completed",reportedUsd:null,estimatedUsd:2,usage:{inputTokens:1,cacheReadTokens:0,cacheWriteTokens:0,outputTokens:2,credits:12,premiumRequests:3},source:"STRUCTURED",origin:"RUNTIME",modelBasis:"CLI_OBSERVED",estimateBasis:"FROZEN",runId:"run",turnId:"one",evidenceDigest:"digest",duplicateSources:[]}],humanEffort:[],knownHumanSeconds:0,unknownHumanEffort:1,unknownDateReceipts:0,legacyUnkeyedReceipts:0} as unknown as DashboardAccounting;
 const html=renderToString(<><AccountingCostCard data={data}/><CounterfactualCard data={data}/><AccountingEvidence data={data}/></>).replace(/<!--.*?-->/g, "");
 expect(html).toContain("—");expect(html).toContain("$0.00");expect(html).toContain("-$1.00");expect(html).toContain("credits 12");expect(html).toContain("premium requests 3");expect(html).toContain("turn:one");expect(html).toContain("digest");expect(html).not.toContain("$12");
});
it("does not display absent dashboard accounting as zero dollars",()=>{expect(renderToString(<AccountingCostCard/>)).not.toContain("$0.00");});
