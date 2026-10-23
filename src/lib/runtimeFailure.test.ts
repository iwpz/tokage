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
 expect(runtimeFailure(s)).toMatchObject({unknownCost:true,models:"planner · Claude · Haiku 5.5"});
 s.runError="scope violation";expect(runtimeFailure(s).unknownCost).toBe(false);s.runError=undefined;
 s.config.blockUnknownCost=false;expect(runtimeFailure(s).unknownCost).toBe(false);
 s.config.blockUnknownCost=true;s.state="Executing";expect(runtimeFailure(s).unknownCost).toBe(false);
});
it("prioritizes unconfirmed cleanup over an older idle-turn error without asserting the process is alive",()=>{
 const s=structuredClone(runtimeUiSnapshot);s.state="Blocked";
 s.turns!.planner.error="native process idle; lost completion cannot be accepted";
 s.turns!.pending={...s.turns!.planner,id:"pending",role:"executor",taskId:"T1",status:"Unknown",ended:false,cleanupConfirmed:false,costUsd:null,error:"interrupted native effect requires independent reconciliation; no automatic relaunch"};
 expect(runtimeFailure(s)).toMatchObject({cleanupUnknown:true,cleanupModels:"T1 · Claude · Haiku 5.5",unknownCost:false});
 s.turns!.pending.ended=true;
 expect(runtimeFailure(s).cleanupUnknown).toBe(true);
 s.turns!.pending.cleanupConfirmed=true;
 expect(runtimeFailure(s).cleanupUnknown).toBe(false);
 s.turns!.pending.cleanupConfirmed=false;s.state="Executing";
 expect(runtimeFailure(s).cleanupUnknown).toBe(false);
});
