import { beforeEach, expect, it, vi } from "vitest";
const calls=vi.hoisted(()=>({start:vi.fn(),resolve:vi.fn(),prepare:vi.fn(),fail:vi.fn(),settle:vi.fn(),cancel:vi.fn(),queue:vi.fn(),undo:vi.fn(),digest:vi.fn(),render:vi.fn(),handler:undefined as ((event: any)=>void)|undefined}));
vi.mock("../lib/ipc",async importOriginal=>({...await importOriginal<object>(),startChatTurn:calls.start,resolveContextInputs:calls.resolve,prepareCompactTurn:calls.prepare,failCompactTurn:calls.fail,settleContextSavings:calls.settle,cancelChatTurn:calls.cancel,queueCompactTurn:calls.queue,undoQueuedCompact:calls.undo,getRepoDigest:calls.digest,renderPlanningPrompt:calls.render,onChatEvent:(cb:any)=>{calls.handler=cb;return Promise.resolve(()=>{});}}));
import { useChat, settleOnLoad, type SendArgs, type Conversation } from "./chat";
import { useSettings } from "./settings";
import { useProjects } from "./projects";
import { IN_WINDOW_CONTEXT_DEFAULTS } from "../lib/contextPlanDefaults";
import { legacyContextInputRequest, type ResolvedContextInputs } from "../lib/contextInputs";
import type { CompactIntent } from "../lib/ipc";
const ref={provider:"CODEX_CLI" as const,model:"gpt"};
function conversation():Conversation {return {thread:{id:"window",ref,mode:"direct"},messages:[{id:"u",role:"user",text:"Implement validation"},{id:"a",role:"assistant",text:"Progress",status:"done",mode:"direct",provider:ref.provider,model:"gpt",variant:{id:"gpt",effort:null,fast:false,tier:null},ref,cwd:"/repo",nativeThreadId:"window",nativeThreadSessionId:"s",endedAt:1000}]};}
function fixture(fresh:boolean):ResolvedContextInputs {
 const req=legacyContextInputRequest(useChat.getState().conversations.s,true,.15,"s")!;
 const intent:CompactIntent={scope:{sessionId:"s",turnId:"a",window:"window",providerName:ref.provider,model:"gpt",role:"direct",taskId:null,revision:null},retained:req.retention!,readOnly:true,requiresExplicitAcceptance:true,payload:{method:fresh?"SYNTHESIZED_FRESH_BRIEF":"INSTRUCTION_PROMPT",prompt:"retained",instruction:"keep"},nextTurn:fresh?{type:"FRESH_WINDOW_BRIEF",brief:"retained"}:{type:"SAME_WINDOW_PROMPT",prefix:"retained"}};
 return {compactIntent:intent,cooldown:{known:true},preview:{recommendedAction:"COMPACT",cooldownRemainingTurns:0}} as ResolvedContextInputs;
}
const args:SendArgs={sessionId:"s",cwd:"/repo",text:"\nUser message exactly  \n",mode:"direct",auto:false,ref,variant:{id:"gpt",effort:null,fast:false,tier:null},modelName:"GPT"};
beforeEach(()=>{
 vi.clearAllMocks();useChat.setState({conversations:{s:conversation()}});
 useSettings.setState({contextMode:"suggest",contextGenerateReport:false,cacheAware:true,contextMinSaving:.15,debugProvider:{output:"off",model:"",provider:""} as never});
 useProjects.setState({projects:[{id:"p",path:"/repo",trusted:true,sessions:[{id:"s"}]} as never]});
 calls.digest.mockResolvedValue({markdown:"repo digest"});calls.render.mockImplementation(({userRequest})=>Promise.resolve(`PLAN:${userRequest}`));
 calls.queue.mockResolvedValue(42);calls.undo.mockResolvedValue(undefined);
 calls.start.mockResolvedValue(undefined);calls.fail.mockResolvedValue(undefined);calls.settle.mockResolvedValue(undefined);calls.cancel.mockResolvedValue(undefined);
 calls.prepare.mockImplementation((_request,_intent,text)=>Promise.resolve({contextPrefix:"retained",userMessage:text,requireNewWindow:_intent.nextTurn.type==="FRESH_WINDOW_BRIEF",readOnly:true,actionId:42}));
});
for(const fresh of [false,true]) it(`sends accepted ${fresh?"fresh brief":"same-window instructions"} through the ordinary transport`,async()=>{
 const inputs=fixture(fresh);calls.resolve.mockResolvedValue(inputs);
 await useChat.getState().queueCompact("s",inputs.compactIntent!);
 expect(calls.start).not.toHaveBeenCalled();
 await useChat.getState().send(args);
 expect(calls.prepare).toHaveBeenCalledTimes(1);
 expect(calls.start).toHaveBeenCalledWith(expect.objectContaining({provider:ref.provider,model:"gpt",resume:fresh?null:"window",fork:false,prompt:`retained\n\n---\n\n${args.text}`,history:fresh?[]:expect.any(Array)}));
 const conv=useChat.getState().conversations.s;
 expect(conv.messages.at(-2)?.text).toBe(args.text);expect(conv.pendingCompact).toBeUndefined();
 expect(calls.prepare.mock.calls[0][3]).toBe(conv.messages.at(-1)?.id);
});
it("rejects changed model, stale retained data, disabled mode and native transport",async()=>{
 const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);
 await useChat.getState().queueCompact("s",inputs.compactIntent!);
 await useChat.getState().send({...args,variant:{...args.variant,id:"another"}});
 expect(calls.start).not.toHaveBeenCalled();expect(calls.prepare).not.toHaveBeenCalled();
 useSettings.setState({contextMode:"off"});await useChat.getState().send(args);expect(calls.start).not.toHaveBeenCalled();
 useSettings.setState({contextMode:"suggest"});
 useChat.setState({conversations:{s:conversation()}});
 const native=fixture(false);native.compactIntent!.nextTurn={type:"NATIVE_COMMAND",command:"/compact",transportReady:false};calls.resolve.mockResolvedValue(native);
 await expect(useChat.getState().queueCompact("s",native.compactIntent!)).rejects.toThrow("Native");
 calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);
 useChat.setState(s=>({conversations:{...s.conversations,s:{...s.conversations.s,messages:s.conversations.s.messages.map(m=>m.id==="u"?{...m,text:"Changed goal"}:m)}}}));
 await useChat.getState().send(args);expect(calls.start).not.toHaveBeenCalled();
});
it("does not consume another session or isolated question; cancellation restores an ordinary send",async()=>{
 const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);
 await useChat.getState().send({...args,isolated:true});expect(calls.prepare).not.toHaveBeenCalled();expect(useChat.getState().conversations.s.pendingCompact).toBeDefined();
 calls.start.mockClear();await useChat.getState().cancelCompact("s");await useChat.getState().send(args);
 expect(calls.start).toHaveBeenCalledWith(expect.objectContaining({prompt:args.text,resume:"window"}));
});
it("preparation failure never sends or reports successful compaction",async()=>{
 const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);
 calls.prepare.mockRejectedValueOnce(new Error("history unavailable"));await useChat.getState().send(args);
 expect(calls.start).not.toHaveBeenCalled();expect(useChat.getState().conversations.s.pendingCompact).toBeDefined();
 expect(useChat.getState().conversations.s.messages.at(-1)?.origin).toBe("tokage");
});
it("never takes a pending compact from another conversation",async()=>{
 const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);
 useChat.setState(s=>({conversations:{...s.conversations,other:conversation()}}));
 await useChat.getState().send({...args,sessionId:"other"});
 expect(calls.prepare).not.toHaveBeenCalled();expect(calls.start).toHaveBeenCalledWith(expect.objectContaining({sessionId:"other",prompt:args.text}));
 expect(useChat.getState().conversations.s.pendingCompact).toBeDefined();
});
it("rejects changed startup flags and preserves the unsent user message",async()=>{
 const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);
 await useChat.getState().send({...args,variant:{...args.variant,effort:"high"}});
 expect(calls.prepare).not.toHaveBeenCalled();expect(calls.start).not.toHaveBeenCalled();
 expect(useChat.getState().conversations.s.queue?.[0].text).toBe(args.text);
});
it("does not send a prepared action after cancellation during IPC",async()=>{
 const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);
 let finish!:(value:unknown)=>void;calls.prepare.mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;}));
 const sending=useChat.getState().send(args);await useChat.getState().cancelCompact("s");
 finish({contextPrefix:"retained",userMessage:args.text,requireNewWindow:true,readOnly:true,actionId:42});await sending;
 expect(calls.fail).toHaveBeenCalledWith(42,expect.any(String),true);
 expect(calls.start).not.toHaveBeenCalled();expect(useChat.getState().conversations.s.queue?.[0].text).toBe(args.text);
});

it("reports queued separately from a completed carrier without claiming native compression",async()=>{
 const inputs=fixture(false);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);
 const queued=useChat.getState().conversations.s.messages.at(-1)!;expect(queued.origin).toBe("tokage");expect(queued.text).toContain("Queued");
 await useChat.getState().send(args);
 const request=calls.start.mock.calls[0][0];
 expect(request.history.some((m:{text:string})=>m.text===queued.text)).toBe(false);
 const carrier=useChat.getState().conversations.s.messages.find(m=>m.compactAction)!;
 expect(carrier.compactAction).toMatchObject({actionId:42,state:"running",method:"instruction"});
 expect(carrier.compactAction?.preparedPayload).toBe("retained");
 calls.handler!({turnId:request.turnId,kind:"done",inputTokens:1,outputTokens:1});
 const conv=useChat.getState().conversations.s;
 expect(conv.messages.find(m=>m.id===carrier.id)?.compactAction?.state).toBe("completed");
 expect(calls.settle).toHaveBeenCalledWith(42,carrier.id,null);
 expect(conv.messages.at(-1)?.text).toContain("unverified");expect(calls.fail).not.toHaveBeenCalled();
});
it("settles a completed fresh carrier with its reported cost",async()=>{
 const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);await useChat.getState().send(args);
 const carrier=useChat.getState().conversations.s.messages.find(m=>m.compactAction)!;
 calls.handler!({turnId:calls.start.mock.calls[0][0].turnId,kind:"done",inputTokens:1,outputTokens:1,costUsd:0.04});
 expect(calls.settle).toHaveBeenCalledWith(42,carrier.id,0.04);
});
it("records a failed carrier when launch rejects or the model reports an error",async()=>{
 const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);
 calls.start.mockRejectedValueOnce(new Error("launch refused"));await useChat.getState().send(args);
 const carrier=useChat.getState().conversations.s.messages.find(m=>m.compactAction)!;
 expect(carrier.compactAction?.state).toBe("failed");expect(calls.fail).toHaveBeenCalledWith(42,carrier.id,false);
 expect(useChat.getState().conversations.s.messages.at(-1)?.text).toContain("failed");
});
it("records a cancelled active carrier and never declares success",async()=>{
 const inputs=fixture(false);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);await useChat.getState().send(args);
 const carrier=useChat.getState().conversations.s.messages.find(m=>m.compactAction)!;
 await useChat.getState().cancel("s");
 expect(useChat.getState().conversations.s.messages.find(m=>m.id===carrier.id)?.compactAction?.state).toBe("cancelled");
 expect(calls.fail).toHaveBeenCalledWith(42,carrier.id,false);expect(useChat.getState().conversations.s.messages.at(-1)?.text).toContain("stopped");
});
it("keeps a failed stream distinct from a completed carrier",async()=>{
 const inputs=fixture(false);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);await useChat.getState().send(args);
 const carrier=useChat.getState().conversations.s.messages.find(m=>m.compactAction)!;
 calls.handler!({turnId:calls.start.mock.calls[0][0].turnId,kind:"error",message:"provider failure"});
 expect(useChat.getState().conversations.s.messages.find(m=>m.id===carrier.id)?.compactAction?.state).toBe("failed");
 expect(calls.fail).toHaveBeenCalledWith(42,carrier.id,false);
 expect(useChat.getState().conversations.s.messages.at(-1)?.text).toContain("unverified");
});
it("does not rewrite a completed carrier when cancellation resolves late",async()=>{
 const inputs=fixture(false);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);await useChat.getState().send(args);
 const turnId=calls.start.mock.calls[0][0].turnId, carrier=useChat.getState().conversations.s.messages.find(m=>m.compactAction)!;
 let release!:()=>void;calls.cancel.mockImplementation(()=>new Promise<void>(resolve=>{release=resolve;}));
 const stopping=useChat.getState().cancel("s");
 calls.handler!({turnId,kind:"done",inputTokens:1,outputTokens:1});release();await stopping;
 expect(useChat.getState().conversations.s.messages.find(m=>m.id===carrier.id)?.compactAction?.state).toBe("completed");
 expect(calls.fail).not.toHaveBeenCalledWith(42,carrier.id,false);
});

it("restoring a conversation clears pending acceptance and leaves an unfinished carrier unverified",()=>{
 const conv=conversation();
 conv.pendingCompact={} as never;
 conv.messages.push({id:"running",role:"assistant",text:"",status:"streaming",compactAction:{actionId:42,method:"instruction",state:"running"}});
 const restored=settleOnLoad(conv);
 expect(restored.pendingCompact).toBeUndefined();
 expect(restored.messages.at(-1)?.compactAction?.state).toBe("unknown");
 expect(restored.messages.at(-1)?.status).toBe("cancelled");
});

it("auto queues and durably binds a recommendation before the next ordinary send without a click",async()=>{
 useSettings.setState({contextMode:"auto"});const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);
 await useChat.getState().send(args);
 expect(calls.queue).toHaveBeenCalledWith(expect.objectContaining({sessionId:"s"}),inputs.compactIntent,true);
 expect(calls.prepare).toHaveBeenCalledWith(expect.anything(),inputs.compactIntent,args.text,expect.any(String),42);
 expect(calls.start).toHaveBeenCalledWith(expect.objectContaining({resume:null,history:[],prompt:`retained\n\n---\n\n${args.text}`}));
});
it("undo records the queued action and suppresses auto requeue for the same source turn",async()=>{
 useSettings.setState({contextMode:"auto"});const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);
 await useChat.getState().queueCompact("s",inputs.compactIntent!,true);
 expect(useChat.getState().conversations.s.pendingCompact).toMatchObject({actionId:42,automatic:true});
 await useChat.getState().cancelCompact("s");
 expect(calls.undo).toHaveBeenCalledWith(42,"s");
 await useChat.getState().queueCompact("s",inputs.compactIntent!,true);
 await useChat.getState().send(args);
 expect(calls.queue).toHaveBeenCalledTimes(1);expect(calls.prepare).not.toHaveBeenCalled();
 expect(calls.start).toHaveBeenCalledWith(expect.objectContaining({resume:"window",prompt:args.text}));
});
it("cancelling during auto queue IPC undoes the late durable suggestion",async()=>{
 useSettings.setState({contextMode:"auto"});const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);
 let finish!:(id:number)=>void;calls.queue.mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;}));
 const queue=useChat.getState().queueCompact("s",inputs.compactIntent!,true);
 await vi.waitFor(()=>expect(calls.queue).toHaveBeenCalled());
 await useChat.getState().cancelCompact("s");finish(88);await queue;
 expect(calls.undo).toHaveBeenCalledWith(88,"s");expect(useChat.getState().conversations.s.pendingCompact).toBeUndefined();
});
it("a second send waits for an auto queue already being persisted",async()=>{
 useSettings.setState({contextMode:"auto"});const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);
 let finish!:(id:number)=>void;calls.queue.mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;}));
 const queue=useChat.getState().queueCompact("s",inputs.compactIntent!,true);
 await vi.waitFor(()=>expect(calls.queue).toHaveBeenCalled());
 const send=useChat.getState().send(args);expect(calls.start).not.toHaveBeenCalled();
 finish(42);await queue;await send;
 expect(calls.queue).toHaveBeenCalledTimes(1);expect(calls.prepare).toHaveBeenCalledTimes(1);
});
it("internal approval-style sends and isolated questions do not auto queue or consume user actions",async()=>{
 useSettings.setState({contextMode:"auto"});const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);
 await useChat.getState().queueCompact("s",inputs.compactIntent!,true);
 await useChat.getState().send({...args,skipContextAction:true});
 expect(calls.prepare).not.toHaveBeenCalled();expect(useChat.getState().conversations.s.pendingCompact).toBeDefined();
});
it("off and suggest do not automatically query or apply recommendations on send",async()=>{
 for(const contextMode of ["off","suggest"] as const) {
  useChat.setState({conversations:{s:conversation()}});useSettings.setState({contextMode});
  await useChat.getState().send(args);
 }
 expect(calls.resolve).not.toHaveBeenCalled();expect(calls.queue).not.toHaveBeenCalled();expect(calls.prepare).not.toHaveBeenCalled();
});
it("unrelated system messages during prepare do not undo a still-current action",async()=>{
 const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);await useChat.getState().queueCompact("s",inputs.compactIntent!);
 let finish!:(v:unknown)=>void;calls.prepare.mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;}));
 const sending=useChat.getState().send(args);
 useChat.getState().postSystemMessage("s","Unrelated diagnostic");
 finish({contextPrefix:"retained",userMessage:args.text,requireNewWindow:true,readOnly:true,actionId:42});await sending;
 expect(calls.start).toHaveBeenCalledTimes(1);expect(calls.fail).not.toHaveBeenCalled();
 expect(useChat.getState().conversations.s.messages.some(m=>m.text==="Unrelated diagnostic")).toBe(true);
});

it("does not install a late suggestion after workspace trust was revoked",async()=>{
 useSettings.setState({contextMode:"auto"});const inputs=fixture(true);calls.resolve.mockResolvedValue(inputs);
 let finish!:(id:number)=>void;calls.queue.mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;}));
 const queue=useChat.getState().queueCompact("s",inputs.compactIntent!,true);
 await vi.waitFor(()=>expect(calls.queue).toHaveBeenCalled());
 useProjects.setState(s=>({projects:s.projects.map(p=>({...p,trusted:false}))}));finish(42);await queue;
 expect(calls.undo).toHaveBeenCalledWith(42,"s");expect(useChat.getState().conversations.s.pendingCompact).toBeUndefined();
});
it("keeps retained material outside a fresh plan's user-request template",async()=>{
 useChat.setState({conversations:{s:{...conversation(),thread:{...conversation().thread!,mode:"plan"},messages:conversation().messages.map(m=>m.role==="assistant"?{...m,mode:"plan"}:m)}}});
 const inputs=fixture(true);inputs.compactIntent!.scope.role="plan";calls.resolve.mockResolvedValue(inputs);
 await useChat.getState().queueCompact("s",inputs.compactIntent!);
 await useChat.getState().send({...args,mode:"plan"});
 expect(calls.render).toHaveBeenCalledWith(expect.objectContaining({userRequest:args.text}));
 expect(calls.start).toHaveBeenCalledWith(expect.objectContaining({resume:null,history:[],prompt:`retained\n\n---\n\nPLAN:${args.text}`}));
});

it("honors a durable undo returned by the resolver after the conversation was reopened",async()=>{
 useSettings.setState({contextMode:"auto"});const inputs={...fixture(true),automaticQueueSuppressed:true};calls.resolve.mockResolvedValue(inputs);
 await useChat.getState().send(args);
 expect(calls.queue).not.toHaveBeenCalled();expect(calls.prepare).not.toHaveBeenCalled();
 expect(calls.start).toHaveBeenCalledWith(expect.objectContaining({prompt:args.text,resume:"window"}));
});

for(const automatic of [false,true]) it(`sends ${automatic?"automatic":"explicit"} FRESH in a new window with its progress prefix`,async()=>{
 useSettings.setState({contextMode:automatic?"auto":"suggest"});
 const inputs=fixture(true);inputs.preview.recommendedAction="FRESH";inputs.compactIntent!.action="FRESH";calls.resolve.mockResolvedValue(inputs);
 if(!automatic) await useChat.getState().queueCompact("s",inputs.compactIntent!);
 await useChat.getState().send(args);
 expect(calls.queue).toHaveBeenCalledWith(expect.anything(),expect.objectContaining({action:"FRESH"}),automatic);
 expect(calls.start).toHaveBeenCalledWith(expect.objectContaining({resume:null,history:[],prompt:`retained\n\n---\n\n${args.text}`}));
 const carrier=useChat.getState().conversations.s.messages.find(m=>m.compactAction)!;
 calls.handler!({turnId:calls.start.mock.calls[0][0].turnId,kind:"done",inputTokens:1,outputTokens:1,costUsd:.05});
 expect(calls.settle).toHaveBeenCalledWith(42,carrier.id,.05);
});

const reportText=["What was completed","Deviations from contracts","Hints for downstream","Self-check against acceptance criteria","Next step"].map(h=>`## ${h}\nRecorded evidence; verify in the next window.`).join("\n");
function warmReportInputs() {
 const inputs=fixture(true);
 inputs.compactIntent!.scope.turnId=inputs.compactIntent!.retained.turnId;
 return {...inputs,warmNow:true,params:{...IN_WINDOW_CONTEXT_DEFAULTS}};
}
async function startReport() {
 useSettings.setState({contextMode:"auto",contextGenerateReport:true});
 calls.resolve.mockImplementation(async()=>warmReportInputs());
 const sending=useChat.getState().send(args);
 await vi.waitFor(()=>expect(calls.start).toHaveBeenCalledTimes(1));
 calls.handler!({turnId:calls.start.mock.calls[0][0].turnId,kind:"thread",id:"window"});
 return {sending,request:calls.start.mock.calls[0][0]};
}
it("generates one warm-window report then sends the untouched request into a fresh window",async()=>{
 const {sending,request}=await startReport();
 expect(request).toMatchObject({resume:"window",phase:"CONTEXT",model:"gpt"});
 expect(request.prompt).toContain("What was completed");
 calls.handler!({turnId:request.turnId,kind:"text",text:reportText});
 calls.handler!({turnId:request.turnId,kind:"done",inputTokens:10,outputTokens:20,costUsd:.03});
 await sending;
 expect(calls.start).toHaveBeenCalledTimes(2);
 expect(calls.start.mock.calls[1][0]).toMatchObject({resume:null,history:[],prompt:`retained\n\n---\n\n${args.text}`});
 const progress=calls.prepare.mock.calls[0][0].retention.progress;
 expect(progress.generatedReport).toMatchObject({sourceTurnId:"a",costUsd:.03,baselineParams:IN_WINDOW_CONTEXT_DEFAULTS});
 expect(progress.modelReport).toBe(reportText);
 expect(useChat.getState().conversations.s.messages.at(-2)?.text).toBe(args.text);
});
for(const failure of ["stop","error","off","trust"] as const) it(`keeps the original message unsent when report ${failure}`,async()=>{
 const {sending,request}=await startReport();
 if(failure==="stop") await useChat.getState().cancel("s");
 else if(failure==="error") calls.handler!({turnId:request.turnId,kind:"error",message:"Report failed"});
 else {
   if(failure==="off") useSettings.setState({contextGenerateReport:false});
   else useProjects.setState({projects:[{id:"p",path:"/repo",trusted:false,sessions:[{id:"s"}]} as never]});
   calls.handler!({turnId:request.turnId,kind:"done",inputTokens:1,outputTokens:1});
 }
 await sending;
 expect(calls.start).toHaveBeenCalledTimes(1);expect(calls.prepare).not.toHaveBeenCalled();
 expect(useChat.getState().conversations.s.queue?.[0].text).toBe(args.text);
});
for(const condition of ["cold","existing","disabled"] as const) it(`skips the extra report when ${condition}`,async()=>{
 useSettings.setState({contextMode:"auto",contextGenerateReport:condition!=="disabled"});
 calls.resolve.mockImplementation(async()=>{
   const inputs=warmReportInputs();
   if(condition==="cold") inputs.warmNow=false;
   if(condition==="existing") inputs.compactIntent!.retained.progress!.modelReport=reportText;
   return inputs;
 });
 await useChat.getState().send(args);
 expect(calls.start).toHaveBeenCalledTimes(1);
 expect(calls.start.mock.calls[0][0].phase).toBeUndefined();
});

it("stop wins over a late completed report while concurrent user input stays queued",async()=>{
 const {sending,request}=await startReport();
 await useChat.getState().send({...args,text:"Second message"});
 let release!:()=>void;
 calls.cancel.mockImplementationOnce(()=>new Promise<void>(resolve=>{release=resolve;}));
 const stopping=useChat.getState().cancel("s");
 calls.handler!({turnId:request.turnId,kind:"done",inputTokens:1,outputTokens:1,costUsd:.02});
 release();await stopping;await sending;
 expect(calls.start).toHaveBeenCalledTimes(1);
 expect(useChat.getState().conversations.s.queue?.map(m=>m.text)).toEqual([args.text,"Second message"]);
});

it("manual acceptance replaces its queued source with the completed report source",async()=>{
 useSettings.setState({contextGenerateReport:true});
 calls.resolve.mockImplementation(async()=>warmReportInputs());
 await useChat.getState().queueCompact("s",warmReportInputs().compactIntent!);
 const sending=useChat.getState().send(args);
 await vi.waitFor(()=>expect(calls.start).toHaveBeenCalledTimes(1));
 const {turnId}=calls.start.mock.calls[0][0];
 calls.handler!({turnId,kind:"thread",id:"window"});
 calls.handler!({turnId,kind:"text",text:reportText});
 calls.handler!({turnId,kind:"done",inputTokens:1,outputTokens:1,costUsd:.03});
 await sending;
 expect(calls.undo).toHaveBeenCalledWith(42,"s");
 expect(calls.queue).toHaveBeenCalledTimes(2);
 expect(calls.start).toHaveBeenCalledTimes(2);
 expect(calls.prepare.mock.calls[0][1].scope.turnId).not.toBe("a");
});
it("stop during old-queue cancellation prevents even the extra report launch",async()=>{
 useSettings.setState({contextGenerateReport:true});
 calls.resolve.mockImplementation(async()=>warmReportInputs());
 await useChat.getState().queueCompact("s",warmReportInputs().compactIntent!);
 let release!:()=>void;
 calls.undo.mockImplementationOnce(()=>new Promise<void>(resolve=>{release=resolve;}));
 const sending=useChat.getState().send(args);
 await vi.waitFor(()=>expect(calls.undo).toHaveBeenCalledTimes(1));
 await useChat.getState().cancel("s");
 release();await sending;
 expect(calls.start).not.toHaveBeenCalled();
 expect(useChat.getState().conversations.s.queue?.[0].text).toBe(args.text);
});

it("a report asking for tool approval stops the handoff instead of counting as completion",async()=>{
 const {sending,request}=await startReport();
 calls.handler!({turnId:request.turnId,kind:"approvalRequest",id:"approval",toolId:"tool",command:"edit file",level:"ask",reason:"Needs approval"});
 await sending;
 expect(calls.start).toHaveBeenCalledTimes(1);expect(calls.prepare).not.toHaveBeenCalled();
 expect(useChat.getState().conversations.s.queue?.[0].text).toBe(args.text);
 expect(useChat.getState().conversations.s.messages.find(m=>m.contextReport)?.status).toBe("waiting_approval");
});
