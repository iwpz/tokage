import {it,expect} from "vitest";
import {formatUsd,formatCostChoice,humanEffortSeconds} from "./accountingDisplay";
it("keeps unknown, zero, estimates and negative counterfactual differences distinct",()=>{
 expect(formatUsd(null)).toBe("—");expect(formatUsd(0)).toBe("$0.00");expect(formatUsd(-.2)).toBe("-$0.20");expect(formatCostChoice(0,1)).toBe("$0.00");expect(formatCostChoice(null,1)).toBe("≈ $1.00");expect(formatUsd(Infinity)).toBe("—");
});
it("records only explicit finite nonnegative human effort",()=>{
 expect(humanEffortSeconds("")).toBeUndefined();expect(humanEffortSeconds("0")).toBe(0);expect(humanEffortSeconds("1.5")).toBe(90);
 for(const value of ["-1","Infinity","NaN","1e99"])expect(()=>humanEffortSeconds(value)).toThrow();
});
