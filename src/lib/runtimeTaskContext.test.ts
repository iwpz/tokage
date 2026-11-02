import { expect, it } from "vitest";
import { taskContextSnapshot, taskContextPreparation } from "../../tests/fixtures/runtime-context";
import { contextCarrierState, taskContextHistory } from "./runtimeTaskContext";
it("scopes history by task, native window origin and identity, preserving incoming FRESH",()=>{
 const s=taskContextSnapshot(), a=s.turns!.a;
 s.turns!.old={...a,id:"old",threadId:"previous-window",windowOrigin:"old",contextPreparation:{...taskContextPreparation,sourceTurn:"old",targetTurn:"old"}};
 s.turns!.b={...a,id:"b",threadId:"new-window",windowOrigin:"b",contextPreparation:{...taskContextPreparation,status:"BOUND",targetTurn:"b",boundAt:1}};
 s.turns!.peer={...a,id:"peer",taskId:"T2",windowOrigin:"b",threadId:"new-window",contextPreparation:{...taskContextPreparation,targetTurn:"peer"}};
 s.execution!.tasks.T1.attempts=["old","a","b","peer"];
 expect(taskContextHistory(s,"T1").rows.map(r=>r.preparation.targetTurn)).toEqual(["b"]);
 expect(taskContextHistory(s,"T2").rows).toEqual([]);
 s.turns!.c={...s.turns!.b,id:"c",windowOrigin:"c",threadId:"different-window",contextPreparation:null};s.execution!.tasks.T1.attempts.push("c");
 expect(taskContextHistory(s,"T1").rows).toEqual([]);
});
it("keeps stale queued records undoable and deduplicates bound task/turn records",()=>{
 const s=taskContextSnapshot(), p=structuredClone(taskContextPreparation);
 s.execution!.tasks.T1.contextActions=[p];s.configHash="changed";
 expect(taskContextHistory(s,"T1").rows[0].stale).toBe(true);
 p.status="BOUND";p.targetTurn="b";p.boundAt=2;
 s.turns!.b={...s.turns!.a,id:"b",contextPreparation:p};s.execution!.tasks.T1.attempts.push("b");
 expect(taskContextHistory(s,"T1").rows).toHaveLength(1);
 s.execution!.tasks.T1.contextActions!.unshift({...p,status:"CANCELLED",targetTurn:"cancelled-before-binding",sourceRevision:1,boundAt:null});
 expect(taskContextHistory(s,"T1").rows.map(r=>r.preparation.targetTurn)).toEqual(["cancelled-before-binding","b"]);
});
it("legacy windows with unknown origin do not get combined by a matching thread id",()=>{
 const s=taskContextSnapshot();s.turns!.a.windowOrigin=null;s.turns!.a.contextPreparation=taskContextPreparation;
 expect(taskContextHistory(s,"T1").rows).toEqual([]);
});
it("carrier status is separate from native confirmation, and cleanup is required",()=>{
 const s=taskContextSnapshot(), a=s.turns!.a;
 expect(contextCarrierState(a)).toBe("completed");expect(taskContextPreparation.nativeConfirmed).toBe(false);
 expect(contextCarrierState({...a,ended:false})).toBe("pending");
 expect(contextCarrierState({...a,cleanupConfirmed:false})).toBe("failed");
 expect(contextCarrierState({...a,status:"Cancelled"})).toBe("failed");
});

it("different roles or observed identities cannot be folded into a task window",()=>{
 const s=taskContextSnapshot(),a=s.turns!.a;
 s.turns!.older={...a,id:"older",observedModel:"different-model",contextPreparation:{...taskContextPreparation,targetTurn:"older"}};
 s.turns!.review={...a,id:"review",role:"reviewer",contextPreparation:{...taskContextPreparation,targetTurn:"review"}};
 s.execution!.tasks.T1.attempts=["older","review","a"];
 expect(taskContextHistory(s,"T1").rows).toEqual([]);
});
