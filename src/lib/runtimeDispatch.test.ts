import {beforeEach,expect,it,vi} from "vitest";
const mocks=vi.hoisted(()=>({invoke:vi.fn(),snapshot:vi.fn(),submit:vi.fn(),refresh:vi.fn(),trusted:true,settings:{debugProvider:{output:"off"},isolation:"worktree",gateEnvAllow:[] as string[],providerPaths:{CODEX_CLI:"/installed/codex"},maxParallel:3,planBudgetUsd:12,pauseOnOverrun:true,agentHardTimeoutMin:30,contextMode:"auto",tokageCoAuthor:true}}));
vi.mock("@tauri-apps/api/core",()=>({invoke:mocks.invoke}));
vi.mock("../stores/projects",()=>({useProjects:{getState:()=>({projects:[{trusted:mocks.trusted,sessions:[{id:"session"}]}]})}}));
vi.mock("../stores/settings",()=>({useSettings:{getState:()=>mocks.settings}}));
vi.mock("../stores/providers",()=>({useProviders:{getState:()=>({providers:[{id:"CODEX_CLI",adapter:{caps:["STRUCTURED_OUTPUT","READ_ONLY_MODE"]}}]})}}));
vi.mock("./models",()=>({currentAvailableModels:()=>[{provider:"CODEX_CLI",cliModelArg:"available"}]}));
vi.mock("./runtime",()=>({runtimeSnapshot:mocks.snapshot,runtimeSubmit:mocks.submit,runtimeRequest:(s:unknown,c:unknown)=>({snapshot:s,command:c})}));
vi.mock("../stores/runtime",()=>({useRuntime:{getState:()=>({refresh:mocks.refresh})}}));
import {createRuntimePlan} from "./runtimeDispatch";
import type {TurnPlan} from "./turn";
const plan:TurnPlan={mode:"plan",auto:false,ref:{provider:"CODEX_CLI",model:"locked"},variant:{id:"locked",effort:"high",fast:true,tier:"priority"},modelName:"Locked model"};
beforeEach(()=>{vi.clearAllMocks();mocks.trusted=true;mocks.settings.debugProvider.output="off";mocks.settings.isolation="worktree";mocks.settings.gateEnvAllow=[];mocks.snapshot.mockResolvedValueOnce(null).mockResolvedValue({runId:"session",revision:1});mocks.invoke.mockResolvedValue({source:"frozen-app"});});
it("freezes ordinary settings and preserves the exact user model lock and parameters",async()=>{
 await createRuntimePlan("session","/repo","Requirement",plan,true);
 expect(mocks.invoke).toHaveBeenCalledWith("runtime_config",{options:expect.objectContaining({concurrency:3,budgetUsd:12,blockUnknownCost:true,timeoutSeconds:1800,models:expect.objectContaining({executor:expect.objectContaining({provider:"CODEX_CLI",requestedModel:"locked",providerPath:"/installed/codex",userLocked:true,modelParams:{effort:"high",fast:true,tier:"priority"}})})})});
 expect(mocks.submit).toHaveBeenNthCalledWith(1,expect.objectContaining({runId:"session",expectedRevision:0,command:{type:"CreateRun",payload:{requirement:"Requirement",config:{source:"frozen-app"}}}}));
 expect(mocks.submit).toHaveBeenNthCalledWith(2,expect.objectContaining({command:{type:"GeneratePlan"}}));
});
it("does not create or redispatch a second engine for an existing run",async()=>{
 mocks.snapshot.mockReset().mockResolvedValue({runId:"session",revision:7});await expect(createRuntimePlan("session","/repo","Requirement",plan,true)).rejects.toThrow("already");expect(mocks.submit).not.toHaveBeenCalled();
});
it("reports unsupported settings before creating a run",async()=>{
 mocks.settings.isolation="inplace";await expect(createRuntimePlan("session","/repo","Requirement",plan,false)).rejects.toThrow("worktree");expect(mocks.submit).not.toHaveBeenCalled();
});

it("does not infer workspace consent while importing or creating a plan",async()=>{mocks.trusted=false;await expect(createRuntimePlan("session","/repo","Requirement",plan,false)).rejects.toThrow("trust");expect(mocks.submit).not.toHaveBeenCalled();expect(mocks.invoke).not.toHaveBeenCalled();});

it("does not silently replace logical model identities through a paid debug surrogate",async()=>{mocks.settings.debugProvider.output="luna";await expect(createRuntimePlan("session","/repo","Requirement",plan,false)).rejects.toThrow("surrogate");expect(mocks.submit).not.toHaveBeenCalled();});
it("imports ordinary Markdown under a fresh identity without generation, approval or execution",async()=>{
 await createRuntimePlan("session","/repo","Requirement",plan,false,"# User supplied ordinary plan");
 expect(mocks.submit.mock.calls.map(call=>call[0].command.type)).toEqual(["CreateRun","ImportPlan"]);
 expect(mocks.submit).toHaveBeenLastCalledWith(expect.objectContaining({command:{type:"ImportPlan",payload:{text:"# User supplied ordinary plan"}}}));
 expect(mocks.refresh).toHaveBeenCalledWith("session");
});
