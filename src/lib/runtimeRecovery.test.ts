import {it,expect} from "vitest";
import {runtimeRecovery} from "./runtimeRecovery";
import {runtimeFailure} from "./runtimeFailure";
import {runtimeUiSnapshot} from "../../tests/fixtures/runtime-ui";
import type {RuntimeSnapshot} from "./runtime";
function blocked(){const s=structuredClone(runtimeUiSnapshot);s.state="Blocked";s.approvedDigest=s.plan!.confirmationDigest;s.execution={sequence:1,tasks:{T1:{state:"BLOCKED",assignment:s.plan!.assignments.T1,attempts:["first","latest"],error:null,deviation:false,repairStep:"Initial",reservationActive:false}}};s.work={attempts:{first:{error:"obsolete failure",gates:[]},latest:{error:null,workspace:{root:"/retained",cwd:"/retained/task",branch:"tokage/task"},gates:[{command:"npm test",cleanupConfirmed:true,result:{passed:false,exitCode:1,outputTail:"assertion failed",errorMessage:"current gate failed"}}]}}};return s;}
it("uses current gate evidence rather than an obsolete attempt and keeps every artifact",()=>{
 const s=blocked();const r=runtimeRecovery(s);expect(r.kind).toBe("gate");expect(runtimeFailure(s).message).toBe("current gate failed");expect(r.artifacts.map(a=>a.id)).toEqual(["first","latest"]);expect(r.actions.retryTasks).toEqual(["T1"]);expect(r.actions.takeoverTasks).toEqual(["T1"]);expect(r.actions.review).toBe(false);
});
it("gates and reservations block mutation even when all model turns ended cleanly",()=>{
 for(const scenario of ["gate","reservation","turn"]){const s=blocked();if(scenario==="gate")s.work!.attempts.latest.gates[0].cleanupConfirmed=false;else if(scenario==="reservation")s.execution!.tasks.T1.reservationActive=true;else s.turns!.planner.cleanupConfirmed=false;
 const r=runtimeRecovery(s);expect(r.kind).toBe("cleanup");expect(r.actions.retryRun).toBe(false);expect(r.actions.retryTasks).toEqual([]);expect(r.actions.takeoverTasks).toEqual([]);expect(r.actions.amend).toBe(false);expect(r.actions.reconcile).toBe(true);}
});
it("retains Git and delivery intents without suggesting replay or human acceptance",()=>{
 for(const scenario of ["commit","delivery"]){const s=blocked();if(scenario==="commit")s.work!.attempts.latest.commitIntent=true;else s.delivery={intent:{completed:false,claimed:true}};
 const r=runtimeRecovery(s);expect(r.kind).toBe("effects");expect(r.actions.takeoverTasks).toEqual([]);expect(r.actions.retryRun).toBe(false);expect(r.actions.reconcile).toBe(true);}
});
it("review requires accepted tasks, exact approval and cleaned effects",()=>{
 const s=blocked();s.execution!.tasks.T1.state="DONE";s.delivery={error:"review refused"};expect(runtimeRecovery(s)).toMatchObject({kind:"review",actions:{review:true,retryRun:false,takeoverTasks:[]}});
 s.approvedDigest="stale";expect(runtimeRecovery(s).actions.review).toBe(false);
 s.approvedDigest=s.plan!.confirmationDigest;s.turns!.planner.cleanupConfirmed=false;expect(runtimeRecovery(s).actions.review).toBe(false);
});
it("stopped and cancellation-pending runs do not imply successful completion",()=>{
 const s=blocked();s.state="CancelRequested";expect(runtimeRecovery(s)).toMatchObject({kind:"cancelling",actions:{retryRun:false,takeoverTasks:[]}});
 s.state="Stopped";expect(runtimeRecovery(s)).toMatchObject({kind:"stopped",actions:{retryRun:true,takeoverTasks:["T1"]}});
 s.state="Executing";expect(runtimeRecovery(s).kind).toBeNull();
});
it("read failures preserve the last state but suppress execution suggestions; worker errors retain reconciliation",()=>{
 const s=blocked();const original=structuredClone(s);const r=runtimeRecovery(s,"repository unavailable or corrupt: disk full");expect(r.kind).toBe("storage");expect(r.actions.retryRun).toBe(false);expect(r.actions.reconcile).toBe(false);expect(s).toEqual(original);
 expect(runtimeRecovery(s,"Runtime transcript gap").kind).toBe("observation");expect(runtimeRecovery(s,undefined,"repository unavailable or corrupt: write failed")).toMatchObject({kind:"storage",actions:{reconcile:true,retryRun:false}});
 expect(runtimeRecovery(s,undefined,undefined,"run owner conflict")).toMatchObject({kind:"owner",actions:{retryRun:false}});
});
it("does not infer missing historical cleanup or workspaces as safe takeover evidence",()=>{
 const s=blocked();delete s.work!.attempts.latest.gates[0].cleanupConfirmed;expect(runtimeRecovery(s).actions.takeoverTasks).toEqual([]);s.work!.attempts.latest.gates=[];delete s.work!.attempts.latest.workspace;expect(runtimeRecovery(s).actions.takeoverTasks).toEqual([]);
 const noPlan={...s,plan:undefined} as RuntimeSnapshot;expect(runtimeRecovery(noPlan).actions.retryRun).toBe(false);
});
it("a failed gate without an error string still explains failure instead of being labelled a cost gap",()=>{
 const s=blocked();s.work!.attempts.latest.gates[0].result!.errorMessage=null;s.turns!.planner.costUsd=null;
 const f=runtimeFailure(s);expect(f.message).toContain("npm test");expect(f.message).toContain("assertion failed");expect(f.unknownCost).toBe(false);
});
it("does not treat a completed task's retained error as another task's current failure",()=>{
 const s=blocked();s.execution!.tasks.T1.state="DONE";s.execution!.tasks.T1.error="old task error";s.runError=undefined;s.delivery={error:"current review refusal"};
 expect(runtimeFailure(s).message).toBe("current review refusal");s.delivery=undefined;
 expect(runtimeFailure(s).message).not.toBe("old task error");
});
