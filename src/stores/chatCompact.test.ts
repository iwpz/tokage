import { beforeEach, expect, it, vi } from "vitest";
const calls=vi.hoisted(()=>({start:vi.fn(),resolve:vi.fn(),prepare:vi.fn()}));
vi.mock("../lib/ipc",async importOriginal=>({...await importOriginal<object>(),startChatTurn:calls.start,resolveContextInputs:calls.resolve,prepareCompactTurn:calls.prepare}));
import { useChat, type SendArgs, type Conversation } from "./chat";
import { useSettings } from "./settings";
import { useProjects } from "./projects";
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
 useSettings.setState({contextMode:"suggest",cacheAware:true,contextMinSaving:.15,debugProvider:{output:"off",model:"",provider:""} as never});
 useProjects.setState({projects:[{id:"p",path:"/repo",trusted:true,sessions:[{id:"s"}]} as never]});
 calls.start.mockResolvedValue(undefined);
 calls.prepare.mockImplementation((_request,_intent,text)=>Promise.resolve({contextPrefix:"retained",userMessage:text,requireNewWindow:_intent.nextTurn.type==="FRESH_WINDOW_BRIEF",readOnly:true}));
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
 calls.start.mockClear();useChat.getState().cancelCompact("s");await useChat.getState().send(args);
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
 const sending=useChat.getState().send(args);useChat.getState().cancelCompact("s");
 finish({contextPrefix:"retained",userMessage:args.text,requireNewWindow:true,readOnly:true});await sending;
 expect(calls.start).not.toHaveBeenCalled();expect(useChat.getState().conversations.s.queue?.[0].text).toBe(args.text);
});
