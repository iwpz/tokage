import { expect, it } from "vitest";
import { runtimeFailure } from "./runtimeFailure";
import { runtimeUiSnapshot } from "../../tests/fixtures/runtime-ui";
it("finds post-task durable failures and delivery errors even with no failed task",()=>{
 const s=structuredClone(runtimeUiSnapshot);s.state="Blocked";s.runError="unknown actual turn cost blocks dispatch";
 expect(runtimeFailure(s).message).toBe(s.runError);
 s.runError=undefined;s.delivery={error:"final review failed"};expect(runtimeFailure(s).message).toBe("final review failed");
});
it("exposes missing-cost evidence for older blocked snapshots without fabricating costs",()=>{
 const s=structuredClone(runtimeUiSnapshot);s.state="Blocked";s.turns!.planner.costUsd=null;
 expect(runtimeFailure(s)).toMatchObject({unknownCost:true,models:"planner · claude-haiku-5-5"});
 s.runError="scope violation";expect(runtimeFailure(s).unknownCost).toBe(false);s.runError=undefined;
 s.config.blockUnknownCost=false;expect(runtimeFailure(s).unknownCost).toBe(false);
 s.config.blockUnknownCost=true;s.state="Executing";expect(runtimeFailure(s).unknownCost).toBe(false);
});
