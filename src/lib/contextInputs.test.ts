import { expect, it } from "vitest";
import { estimateVisibleTokens, legacyContextInputRequest, runtimeContextInputRequest } from "./contextInputs";
import { runtimeUiSnapshot, runtimeUiTranscripts } from "../../tests/fixtures/runtime-ui";
import type { ChatMessage, Conversation } from "../stores/chat";

it("uses the durable CLI context receipt, including zero, never cumulative input",()=>{
 const s=structuredClone(runtimeUiSnapshot), turn=s.turns!.planner;
 turn.usage={inputTokens:999999,outputTokens:20,contextTokens:0,apiCalls:4};
 const r=runtimeContextInputRequest(s,{},["planner"],true,.15)!;
 expect(r.observations[0]).toMatchObject({contextTokens:0,apiCalls:4,outputTokens:20});
 turn.usage.contextTokens=null;
 expect(runtimeContextInputRequest(s,{},["planner"],true,.15)!.observations[0].contextTokens).toBeNull();
});
it("reads the transcript receipt when durable usage is absent",()=>{
 const s=structuredClone(runtimeUiSnapshot), rows=structuredClone(runtimeUiTranscripts);
 rows.planner.push({turnId:"planner",sequence:99,event:{kind:"done",inputTokens:100000,outputTokens:12,contextTokens:1234,apiCalls:3,costUsd:null,premiumRequests:null}});
 expect(runtimeContextInputRequest(s,rows,["planner"],true,.15)!.observations[0]).toMatchObject({contextTokens:1234,apiCalls:3});
});
it("isolates native thread, model, role, task and incomplete turns",()=>{
 const s=structuredClone(runtimeUiSnapshot), base=s.turns!.planner;
 base.finishedAt=10;
 s.turns!.same={...base,id:"same",finishedAt:20};
 s.turns!.peer={...base,id:"peer",threadId:"other",finishedAt:15};
 s.turns!.role={...base,id:"role",role:"reviewer",finishedAt:16};
 s.turns!.task={...base,id:"task",taskId:"T1",finishedAt:17};
 s.turns!.model={...base,id:"model",requested:{...base.requested,requestedModel:"another"},finishedAt:18};
 s.turns!.active={...base,id:"active",ended:false,finishedAt:30};
 const r=runtimeContextInputRequest(s,{},["same","peer","planner","role","task","model","active"],true,.15)!;
 expect(r.observations.map(o=>o.id)).toEqual(["planner","same"]);
});
it("does not combine legacy messages without durable window identities or fabricate zero",()=>{
 const messages:ChatMessage[]=[{id:"old",role:"assistant",text:"x",status:"done",provider:"CLAUDE_CODE",model:"sonnet",endedAt:1000,usage:{input:999999,output:0}},{id:"user",role:"user",text:"你好"},{id:"new",role:"assistant",text:"hello",status:"done",provider:"CLAUDE_CODE",model:"sonnet",endedAt:2000,usage:{input:999999,output:0}}];
 const conv={messages,thread:null} as Conversation;
 expect(legacyContextInputRequest(conv,true,.15)!.observations).toHaveLength(1);
 expect(legacyContextInputRequest(conv,true,.15)!.observations[0]).toMatchObject({id:"new",contextTokens:null,outputTokens:0});
 expect(estimateVisibleTokens("你好abcd")).toBe(2);
});

it("does not promote estimated usage into CLI-reported statistics",()=>{
 const s=structuredClone(runtimeUiSnapshot);
 s.turns!.planner.usage={inputTokens:999,outputTokens:10,contextTokens:888,apiCalls:3,source:"Estimated"};
 expect(runtimeContextInputRequest(s,{},["planner"],true,.15)!.observations[0]).toMatchObject({contextTokens:null,outputTokens:null,apiCalls:null});
});

it("prefers CLI-observed model identity and isolates a changed actual model",()=>{
 const s=structuredClone(runtimeUiSnapshot),first=s.turns!.planner;
 first.finishedAt=1;first.observedModel="model-before";
 s.turns!.last={...first,id:"last",finishedAt:2,observedModel:"model-actual"};
 const r=runtimeContextInputRequest(s,{},["planner","last"],true,.15)!;
 expect(r.model).toBe("model-actual");expect(r.modelIdentity).toBe("CLI_OBSERVED");expect(r.observations.map(o=>o.model)).toEqual(["model-actual"]);
});
