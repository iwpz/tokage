import { beforeEach, expect, it, vi } from "vitest";

const mock = vi.hoisted(() => ({
  conversations:{} as Record<string,import("../stores/chat").Conversation>,
  picks: {direct: {provider:"AIDER",model:"deepseek/deepseek-flash"} as {provider:string;model:string}|null, plan:null, fixed:null},
  assign: vi.fn(), record: vi.fn(), triage: vi.fn(),
}));
vi.mock("../stores/chat", () => ({useChat: {getState: () => ({picks:mock.picks,modelOptions:{},conversations:mock.conversations})}}));
vi.mock("../stores/providers", () => ({useProviders: {getState: () => ({providers:[{id:"AIDER",status:"READY"},{id:"OPENCODE",status:"READY"}]})}}));
vi.mock("../stores/settings", () => ({useSettings: {getState: () => ({providerEnabled:{},providerTrust:{},blockedModels:[],plannerModel:"auto",modelBanningLevel:"off",modelProviderPreferences:{},modelChannelAccess:{},referenceModel:"auto"})}}));
vi.mock("./models", () => {
  const groups=["AIDER","OPENCODE"].map(id=>({provider:{id},models:[{key:"deepseek/deepseek-flash",name:"DeepSeek Flash",variants:[]}]}));
  return {
    selectableGroups:()=>groups,
    findModel:(items:typeof groups,ref:{provider:string;model:string})=>items.find(g=>g.provider.id===ref.provider)?.models.find(m=>m.key===ref.model),
    recommend:()=>({provider:"OPENCODE",model:"deepseek/deepseek-flash"}),
    resolveVariant:(m:{key:string})=>({id:m.key,effort:null,fast:false,tier:null}),
    optionSummary:()=>[],refKey:(r:{provider:string;model:string})=>`${r.provider}:${r.model}`,
    currentAvailableModels:()=>[],triage:()=>"direct",
  };
});
vi.mock("./ipc", () => ({
  assignDirectTask:mock.assign,
  calculateDirectCounterfactualCost:vi.fn().mockResolvedValue(0.01),
  getProviderUtilizationNow:vi.fn().mockResolvedValue({utilizationPct:null}),
  recordSavingsEvent:mock.record,
  triageRequest:mock.triage,
}));
vi.mock("./demoIdentity", () => ({demoIdentity:()=>false}));
vi.mock("./debugProvider", () => ({builtinDebugCatalog:()=>[]}));
vi.mock("../i18n", () => ({default:{t:(s:string)=>s}}));
import { planTurnAsync, settleDirectSavings } from "./turn";

beforeEach(()=>{
  vi.clearAllMocks();
  mock.conversations={};
  mock.picks.direct={provider:"AIDER",model:"deepseek/deepseek-flash"};
  mock.assign.mockResolvedValue({provider:"OPENCODE",cliModelArg:"deepseek/deepseek-flash",displayName:"Flash",estCostUsd:0.001});
  mock.record.mockResolvedValue(1);
  mock.triage.mockResolvedValue({route:"PLAN",detectedDomains:[],scorePlan:10,scoreDirect:0});
});

it("SR2 continues recovered startup flags without replacing them with global defaults",async()=>{
 const ref={provider:"AIDER" as const,model:"deepseek/deepseek-flash"};
 mock.conversations.s={messages:[],thread:{id:"native",ref,mode:"direct"},directModel:null,recoveredStartup:{ref,variant:{id:ref.model,effort:"high",fast:true,tier:"balance"}}};
 const plan=await planTurnAsync("continue","direct","/repo",null,"s");
 expect(plan?.variant).toEqual({id:ref.model,effort:"high",fast:true,tier:"balance"});
 mock.conversations.s.recoveredStartup!.variant.id="removed-native-model";
 expect(await planTurnAsync("continue","direct","/repo",null,"s")).toBeNull();
});

it("honors a selectable explicit direct provider even if the matcher would choose another CLI",async()=>{
  const plan=await planTurnAsync("small request","direct","/repo",null,"s");
  expect(plan?.ref).toEqual({provider:"AIDER",model:"deepseek/deepseek-flash"});
  expect(mock.assign).not.toHaveBeenCalled();
});

it("uses the visible local recommendation when direct mode has no manual pick",async()=>{
  mock.picks.direct=null;
  const plan=await planTurnAsync("small request","direct","/repo",null,"s");
  expect(plan?.ref.provider).toBe("OPENCODE");
  expect(mock.assign).not.toHaveBeenCalled();
});

it("SA1 never promotes complex direct requests into plan orchestration",async()=>{
  const plan=await planTurnAsync("Build a complete new application with many modules","direct","/repo",null,"s");
  expect(plan?.mode).toBe("direct");
  expect(mock.triage).not.toHaveBeenCalled();
});

it("SA1 fixed resources are an explicit planning strategy, not automatic triage",async()=>{
  const plan=await planTurnAsync("Explain this one line","fixed","/repo",null,"s");
  expect(plan?.mode).toBe("plan");
  expect(mock.triage).not.toHaveBeenCalled();
  expect(mock.assign).not.toHaveBeenCalled();
});

it("keeps missing cost unknown, while preserving a reported zero",async()=>{
  await settleDirectSavings("s",{baselineUsd:1,domain:"general"},null);
  expect(mock.record).toHaveBeenLastCalledWith(expect.objectContaining({actualUsd:undefined}));
  await settleDirectSavings("s",{baselineUsd:1,domain:"general"},0);
  expect(mock.record).toHaveBeenLastCalledWith(expect.objectContaining({actualUsd:0}));
});
