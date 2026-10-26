import { expect, it } from "vitest";
import { runtimeRetentionContext, legacyRetentionContext } from "./compactContext";
import { runtimeContextInputRequest } from "./contextInputs";
import { runtimeUiSnapshot } from "../../tests/fixtures/runtime-ui";
import type { Conversation, ChatMessage } from "../stores/chat";

it("retains the current task goal, embedded contracts and exact failing attempt errors",()=>{
 const s=structuredClone(runtimeUiSnapshot),turn=s.turns!.planner;
 turn.role="executor";turn.taskId="T1";turn.status="Failed";turn.error="Current turn failure";
 s.execution={sequence:1,tasks:{T1:{state:"BLOCKED",assignment:turn.requested,attempts:[turn.id],error:"Task blocked",deviation:false,repairStep:"NONE"}}};
 s.work={attempts:{[turn.id]:{error:"Gate rejected",gates:[{result:{passed:false,command:"node --test",outputTail:"assertion failed"}}]},peer:{error:"FOREIGN_ERROR",gates:[]}}};
 const request=runtimeContextInputRequest(s,{},[turn.id],true,.15)!;
 expect(request.provider).toBe(turn.observedProvider);expect(request.retention).toMatchObject({sessionId:"run",turnId:"planner",taskId:"T1",taskGoal:s.plan!.plan.tasks[0].goal,goalSource:"ACTIVE_TASK_GOAL",contractPaths:[]});
 expect(request.retention!.contracts).toEqual(s.plan!.plan.contracts);
 expect(request.retention!.pendingErrors.join("\n")).toContain("assertion failed");expect(request.retention!.pendingErrors.join("\n")).not.toContain("FOREIGN_ERROR");
});
it("main sessions retain their persisted goal and current errors without inventing paths",()=>{
 const s=structuredClone(runtimeUiSnapshot),turn=s.turns!.planner;
 s.state="Blocked";s.runError="Current run error";
 const context=runtimeRetentionContext(s,turn)!;
 expect(context.taskGoal).toBe(s.plan!.plan.spec.goal);expect(context.contractPaths).toEqual([]);expect(context.pendingErrors).toContain("Current run error");
 delete s.plan;expect(runtimeRetentionContext(s,turn)!.taskGoal).toBe(s.requirement);
});
it("successful recovery does not carry old failures and busy or incomplete sources are unavailable",()=>{
 const s=structuredClone(runtimeUiSnapshot),turn=s.turns!.planner;
 s.runError="HISTORICAL_ERROR";expect(runtimeRetentionContext(s,turn)!.pendingErrors).toEqual([]);
 s.turns!.active={...turn,id:"active",ended:false};expect(runtimeRetentionContext(s,turn)).toBeNull();
 delete s.turns!.active;turn.taskId="MISSING";expect(runtimeRetentionContext(s,turn)).toBeNull();
 turn.taskId="T1";s.plan!.plan.tasks[0].contracts=["MISSING_CONTRACT"];expect(runtimeRetentionContext(s,turn)).toBeNull();
});
it("legacy retention uses the real session goal and only errors since the last successful turn",()=>{
 const base:ChatMessage={id:"a",role:"assistant",mode:"direct",text:"",status:"error",error:"OLD_ERROR",endedAt:1000,provider:"CODEX_CLI",model:"gpt",nativeThreadId:"w",nativeThreadSessionId:"s"};
 const done={...base,id:"b",status:"done" as const,error:undefined,endedAt:2000};
 const failed={...base,id:"c",error:"CURRENT_ERROR",endedAt:3000};
 const conv={thread:{id:"w",ref:{provider:"CODEX_CLI",model:"gpt"},mode:"direct"},messages:[{id:"u",role:"user",text:"真实目标"},base,done,failed]} as Conversation;
 expect(legacyRetentionContext(conv,failed,"s")).toMatchObject({taskGoal:"真实目标",pendingErrors:["CURRENT_ERROR"],contractPaths:[]});
 expect(legacyRetentionContext(conv,failed,"other-session")).toBeNull();
});
it("source revision and role follow the selected window rather than an unrelated peer",()=>{
 const s=structuredClone(runtimeUiSnapshot),turn=s.turns!.planner;s.revision=42;
 const request=runtimeContextInputRequest(s,{},[turn.id],true,.15)!;
 expect(request).toMatchObject({sessionId:s.runId,role:turn.role,taskId:null,revision:42});
 expect(request.retention).toMatchObject({sessionId:s.runId,turnId:turn.id,role:turn.role,revision:42});
});
