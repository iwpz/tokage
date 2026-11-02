import {expect,it,vi} from "vitest";
import {projectDirectRecovery} from "./directProjection";
import {useSettings} from "../stores/settings";
import type {Conversation} from "../stores/chat";
import type {DirectSnapshot} from "./direct";

function snapshot():DirectSnapshot{
  return {sessionId:"s",active:false,tip:"t",approvals:[],turns:[{record:{schemaVersion:1,turnId:"t",sessionId:"s",cwd:"/repo",status:"completed",createdAt:100,finishedAt:105,error:null,
    request:{cwd:"/repo",prompt:"Actual framed input",provider:"CODEX_CLI",model:"variant",effort:"high",fast:true,tier:null,sessionAllowed:["never restore this grant"],
      conversation:{userMessageId:"u",assistantMessageId:"a",userText:"Visible request",ref:{provider:"CODEX_CLI",model:"grouped"},projectPath:"/repo",sessionTitle:"Original title"}}},partialTail:false,
    events:[{kind:"thread",id:"native"},{kind:"model",name:"observed"},{kind:"toolStart",id:"tool",tool:"command",title:"check",detail:null},{kind:"toolEnd",id:"tool",ok:true,output:"passed"},
      {kind:"fileChange",toolId:"tool",path:"f.ts",absPath:"/repo/f.ts",previewPath:null,change:"modified",diff:"diff",additions:1,deletions:1},
      {kind:"text",text:"Saved answer"},{kind:"done",inputTokens:10,outputTokens:5,costUsd:0,contextTokens:0,premiumRequests:null,source:"STRUCTURED"}]}]};
}

it("reconstructs display/tool/file/usage state and the actual native window without learning or granting",()=>{
  const learning=vi.spyOn(useSettings.getState(),"recordObservedModel");
  try{
    const recovered=projectDirectRecovery(undefined,snapshot());
    expect(recovered.messages.map(m=>m.id)).toEqual(["u","a"]);
    expect(recovered.messages[0].text).toBe("Visible request");
    expect(recovered.messages[1]).toMatchObject({text:"Saved answer",turnId:"t",status:"done",reportedModel:"observed",variant:{id:"variant",effort:"high",fast:true},usage:{costUsd:0,contextTokens:0},files:[{path:"f.ts"}]});
    expect(recovered.messages[1].parts).toContainEqual(expect.objectContaining({type:"tool",ok:true,output:"passed",files:["f.ts"]}));
    expect(recovered.thread).toEqual({id:"native",ref:{provider:"CODEX_CLI",model:"grouped"},mode:"direct"});
    expect(recovered.sessionAllowed).toBeUndefined();
    expect(learning).not.toHaveBeenCalled();
    expect(projectDirectRecovery(recovered,snapshot())).toEqual(recovered);
  }finally{learning.mockRestore();}
});

it("fills a partial UI snapshot without duplicating messages or draining its queue",()=>{
  const original:Conversation={thread:null,directTip:"t",sessionAllowed:["existing"],queue:[{id:"queued",text:"do later",mode:"direct",createdAt:1}],messages:[{id:"u",role:"user",text:"Visible request"},{id:"a",role:"assistant",turnId:"t",text:"Saved",status:"error"}]};
  const serialized=JSON.stringify(original);
  const recovered=projectDirectRecovery(original,snapshot());
  expect(recovered.messages).toHaveLength(2);expect(recovered.messages[1].text).toBe("Saved answer");
  expect(recovered.queue).toBe(original.queue);expect(recovered.sessionAllowed).toEqual(["existing"]);
  expect(JSON.stringify(original)).toBe(serialized);
});

it("does not accept provisional Done as success for an interrupted or truncated turn",()=>{
  const input=snapshot();input.turns[0].record.status="running";input.turns[0].record.finishedAt=null;input.turns[0].partialTail=true;
  input.turns[0].events.splice(3,1);
  const recovered=projectDirectRecovery(undefined,input);
  expect(recovered.recoveryUnresolved).toBe(true);expect(recovered.messages[1].status).toBe("error");
  expect(recovered.messages[1].parts).toContainEqual(expect.objectContaining({type:"warning"}));
  expect(recovered.messages[1].parts?.find(p=>p.type==="tool")).toMatchObject({ok:null});
});

it("restores approval audit only as evidence, without creating command consent",()=>{
  const input=snapshot();input.turns[0].record.status="awaiting_approval";
  input.turns[0].events=[{kind:"approvalRequest",id:"permission",toolId:"cmd",command:"touch f",level:"project",reason:"sandbox_denied",recoverable:true}];
  const pending=projectDirectRecovery(undefined,input);
  expect(pending.messages[1].status).toBe("waiting_approval");
  input.approvals=[{id:1,turnId:"different",command:"touch f",level:"project",decision:"allow_session",mode:"ask_sensitive"}];
  expect(projectDirectRecovery(undefined,input).messages[1].status).toBe("waiting_approval");
  input.approvals[0].turnId="t";
  const restored=projectDirectRecovery(undefined,input);
  expect(restored.messages[1].parts?.[0]).toMatchObject({resolved:true,decision:"allow_session"});
  expect(restored.messages[1].status).toBe("error");expect(restored.sessionAllowed).toBeUndefined();
  input.turns[0].events.push({...input.turns[0].events[0],id:"another"} as never);
  expect(projectDirectRecovery(undefined,input).messages[1].status).toBe("waiting_approval");
});

it("rejects active owners and ambiguous legacy matches without modifying the UI",()=>{
  const input=snapshot();input.active=true;expect(()=>projectDirectRecovery(undefined,input)).toThrow();input.active=false;
  delete input.turns[0].record.request.conversation;input.turns[0].record.request.prompt="same";
  const original:Conversation={thread:null,messages:[{id:"u1",role:"user",text:"same"},{id:"a1",role:"assistant",mode:"direct",provider:"CODEX_CLI",model:"variant",text:"Saved"},{id:"u2",role:"user",text:"same"},{id:"a2",role:"assistant",mode:"direct",provider:"CODEX_CLI",model:"variant",text:"Saved"}]};
  const serialized=JSON.stringify(original);expect(()=>projectDirectRecovery(original,input)).toThrow();expect(JSON.stringify(original)).toBe(serialized);
});

it("keeps isolated windows out of the main continuation and does not reuse a failed new resource's predecessor",()=>{
  const input=snapshot();const isolated=structuredClone(input.turns[0]);isolated.record.turnId="isolated";isolated.record.request.conversation!.assistantMessageId="ia";isolated.record.request.conversation!.userMessageId="iu";isolated.record.request.conversation!.isolated=true;isolated.events=[{kind:"thread",id:"one-off"},...isolated.events.filter(e=>e.kind!=="thread")];input.turns.push(isolated);input.tip="isolated";
  expect(projectDirectRecovery(undefined,input).thread?.id).toBe("native");
  input.turns.pop();input.tip="t";input.turns[0].events=[];input.turns[0].record.status="failed";input.turns[0].record.request.resume=null;
  const restored=projectDirectRecovery({messages:[],thread:{id:"old",ref:{provider:"AIDER",model:"old"}},directModel:{provider:"AIDER",model:"old"}},input);
  expect(restored.thread).toBeNull();expect(restored.directModel).toEqual({provider:"CODEX_CLI",model:"grouped"});
});

it("retains saved plain-text context without inventing earlier tool results or model identity",()=>{
  const input=snapshot();input.turns[0].record.request.history=[{role:"user",text:"Original goal"},{role:"assistant",text:"Earlier progress"}];
  const restored=projectDirectRecovery(undefined,input);
  expect(restored.messages.map(m=>m.text)).toEqual(["Original goal","Earlier progress","Visible request","Saved answer"]);
  expect(restored.messages[1].usage).toBeUndefined();expect(restored.messages[1].model).toBeUndefined();
  expect(projectDirectRecovery(restored,input).messages).toHaveLength(4);
});

it("deduplicates an unambiguous legacy pair and labels raw input when original UI text is unavailable",()=>{
 const input=snapshot();delete input.turns[0].record.request.conversation;input.turns[0].record.request.prompt="Visible request";
 const original:Conversation={messages:[{id:"old-user",role:"user",text:"Visible request"},{id:"old-answer",role:"assistant",text:"Saved",mode:"direct",provider:"CODEX_CLI",model:"variant",ref:{provider:"CODEX_CLI",model:"grouped"}}],thread:null};
 const restored=projectDirectRecovery(original,input);
 expect(restored.messages.map(m=>m.id)).toEqual(["old-user","old-answer"]);
 expect(restored.messages[1].text).toBe("Saved answer");
 expect(restored.thread).toBeNull();
 expect(restored.recoveredStartup?.ref.model).toBe("grouped");
 expect(projectDirectRecovery(undefined,input).messages[0].recoveredRawInput).toBe(true);
});


it("restores frozen startup parameters at launch even when the durable tip was already persisted",()=>{
 const input=snapshot();const base:Conversation={messages:[],thread:null,directTip:input.tip};
 expect(projectDirectRecovery(base,input).recoveredStartup).toBeUndefined();
 expect(projectDirectRecovery(base,input,true).recoveredStartup?.variant.id).toBe("variant");
});
