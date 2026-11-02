import {beforeEach,expect,it,vi} from "vitest";
const calls=vi.hoisted(()=>({snapshot:vi.fn(),git:vi.fn(),start:vi.fn(),approve:vi.fn(),reconcile:vi.fn()}));
vi.mock("./direct",()=>({directRecovery:calls.snapshot,directReconcile:calls.reconcile}));
vi.mock("./ipc",async original=>({...await original<object>(),inTauri:true,gitInfo:calls.git,startChatTurn:calls.start,resolveApproval:calls.approve}));
import {restoreDirectSession,restoreExistingDirectSessions} from "./directRecovery";
import {useProjects} from "../stores/projects";
import {useChat} from "../stores/chat";
import {useSettings} from "../stores/settings";
import type {DirectSnapshot} from "./direct";

function fixture():DirectSnapshot{
 return {sessionId:"s",active:false,tip:"native-turn",approvals:[],turns:[{partialTail:false,record:{schemaVersion:1,turnId:"native-turn",sessionId:"s",cwd:"/repo",createdAt:100,finishedAt:101,status:"completed",error:null,
 request:{cwd:"/repo",prompt:"actual input",provider:"CODEX_CLI",model:"concrete",sessionAllowed:["not a restored grant"],conversation:{userMessageId:"u",assistantMessageId:"a",userText:"Visible goal",ref:{provider:"CODEX_CLI",model:"group"},projectPath:"/repo",sessionTitle:"Saved title"}}},
 events:[{kind:"thread",id:"native-window"},{kind:"text",text:"Actual saved result"},{kind:"done",inputTokens:1,outputTokens:2,costUsd:null,premiumRequests:null}]}]};
}
beforeEach(()=>{
 vi.clearAllMocks();calls.snapshot.mockResolvedValue(fixture());
 useProjects.setState({projects:[{id:"p",name:"Project",path:"/repo",expanded:true,trusted:false,sessions:[]}],activeProjectId:"p",activeSessionId:null});
 useChat.setState({conversations:{}});
});
it("discovers a missing sidebar identity and restores it without consent or inference",async()=>{
 const options=useChat.getState().modelOptions;const settings=useSettings.getState();
 await restoreDirectSession("p","s",true);
 expect(useProjects.getState().projects[0]).toMatchObject({trusted:false,sessions:[{id:"s",title:"Saved title"}]});
 expect(useProjects.getState().activeSessionId).toBe("s");
 expect(useChat.getState().conversations.s.messages[1].text).toBe("Actual saved result");
 expect(useChat.getState().conversations.s.thread?.id).toBe("native-window");
 expect(useChat.getState().conversations.s.sessionAllowed).toBeUndefined();
 expect(useChat.getState().modelOptions).toBe(options);expect(useSettings.getState()).toBe(settings);
 expect(calls.start).not.toHaveBeenCalled();expect(calls.approve).not.toHaveBeenCalled();expect(calls.reconcile).not.toHaveBeenCalled();
});
it("hydrates only existing non-archived sessions and never automatically resurrects a deleted row",async()=>{
 useProjects.setState(s=>({projects:s.projects.map(p=>({...p,sessions:[{id:"s",title:"Renamed",state:"draft",updatedAt:1},{id:"archived",title:"Archived",state:"draft",updatedAt:1,archived:true}]}))}));
 await restoreExistingDirectSessions();
 expect(calls.snapshot).toHaveBeenCalledTimes(1);expect(calls.snapshot).toHaveBeenCalledWith("s","p","/repo","/repo");
 expect(useProjects.getState().projects[0].sessions[0].title).toBe("Renamed");
 expect(useProjects.getState().activeSessionId).toBeNull();
 expect(calls.start).not.toHaveBeenCalled();
});
it("preserves an interrupted queue and blocks continuation until explicit reconciliation",async()=>{
 const input=fixture();input.turns[0].record.finishedAt=null;input.turns[0].record.status="running";calls.snapshot.mockResolvedValue(input);
 const queue=[{id:"q",text:"queued work",mode:"direct" as const,createdAt:1}];
 useChat.setState({conversations:{s:{messages:[],thread:null,queue}}});
 await restoreDirectSession("p","s");
 expect(useChat.getState().conversations.s.queue).toBe(queue);
 expect(useChat.getState().conversations.s.recoveryUnresolved).toBe(true);
 expect(useChat.getState().conversations.s.messages[1].status).toBe("error");
 expect(calls.reconcile).not.toHaveBeenCalled();expect(calls.start).not.toHaveBeenCalled();
});
it("does not overwrite live owners or edits made while the snapshot was loading",async()=>{
 const input=fixture();input.active=true;calls.snapshot.mockResolvedValue(input);
 const before=useProjects.getState().projects;
 await expect(restoreDirectSession("p","s")).rejects.toThrow();expect(useProjects.getState().projects).toBe(before);
 let resolve!:(snapshot:DirectSnapshot)=>void;calls.snapshot.mockImplementation(()=>new Promise<DirectSnapshot>(r=>{resolve=r;}));
 const loading=restoreDirectSession("p","s");
 useChat.setState({conversations:{s:{messages:[{id:"new",role:"user",text:"New edit"}],thread:null}}});
 resolve(fixture());await expect(loading).rejects.toThrow();
 expect(useChat.getState().conversations.s.messages[0].text).toBe("New edit");
 expect(useProjects.getState().projects).toBe(before);
});
it("reattaches a recorded worktree without creating one or granting its old permissions",async()=>{
 const input=fixture();input.turns[0].record.cwd="/worktree";input.turns[0].record.request.cwd="/worktree";
 input.turns[0].record.request.conversation!.worktree={root:"/worktree",cwd:"/worktree",branch:"existing-branch"};calls.snapshot.mockResolvedValue(input);
 await restoreDirectSession("p","s",true);
 expect(useProjects.getState().projects[0].sessions[0].worktree).toEqual({root:"/worktree",cwd:"/worktree",branch:"existing-branch"});
 expect(calls.git).not.toHaveBeenCalled();expect(calls.start).not.toHaveBeenCalled();
});

it("waits for a new user send before resuming the recovered native window",async()=>{
 await restoreDirectSession("p","s",true);
 expect(calls.start).not.toHaveBeenCalled();
 const recovered=useChat.getState().conversations.s;
 const message=recovered.messages.at(-1)!;
 calls.start.mockResolvedValue(undefined);
 await useChat.getState().send({sessionId:"s",cwd:"/repo",text:"My new instruction",mode:"direct",auto:false,ref:message.ref!,variant:message.variant!,modelName:"Restored"});
 expect(calls.start).toHaveBeenCalledOnce();
 expect(calls.start).toHaveBeenCalledWith(expect.objectContaining({resume:"native-window",prompt:"My new instruction",directTip:"native-turn",model:"concrete",conversation:expect.objectContaining({userText:"My new instruction"})}));
 await useChat.getState().cancel("s");
});

it("explicitly restores archived rows without renaming them or restoring their old trust",async()=>{
 useProjects.setState(s=>({projects:s.projects.map(p=>({...p,sessions:[{id:"s",title:"User title",state:"draft",updatedAt:1,archived:true}]}))}));
 await restoreDirectSession("p","s",true);
 expect(useProjects.getState().projects[0]).toMatchObject({trusted:false,sessions:[{id:"s",title:"User title",archived:false}]});
});
