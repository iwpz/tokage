import {beforeEach,expect,it,vi} from "vitest";
import {upgradeDirectSession} from "./planUpgrade";
const m=vi.hoisted(()=>({conv:{messages:[{id:"u",role:"user",text:"goal"},{id:"a",role:"assistant",text:"done",status:"done"}],thread:null},git:vi.fn(),history:vi.fn(),create:vi.fn(),plan:vi.fn(),setChat:vi.fn(),trusted:true}));
vi.mock("../stores/chat",()=>({useChat:{getState:()=>({conversations:{source:m.conv}}),setState:m.setChat},isStreaming:()=>false}));
vi.mock("../stores/projects",()=>({useProjects:{getState:()=>({projects:[{id:"p",trusted:m.trusted,sessions:[{id:"source"}],path:"/repo"}],createSession:m.create})},sessionCwd:()=>"/repo"}));
vi.mock("./direct",()=>({directHistory:m.history}));
vi.mock("./ipc",()=>({gitInfo:m.git}));
vi.mock("./turn",()=>({planTurn:()=>({mode:"plan",ref:{provider:"CODEX_CLI",model:"planner"}})}));
vi.mock("./runtimeDispatch",()=>({createRuntimePlan:m.plan}));
vi.mock("../i18n",()=>({default:{t:(key:string)=>key}}));
beforeEach(()=>{vi.clearAllMocks();m.trusted=true;m.git.mockResolvedValue({isRepo:true,hasCommits:true,detached:false,changes:0});m.history.mockResolvedValue([]);m.create.mockReturnValue("linked");m.plan.mockResolvedValue(undefined);});
it("creates a different linked run with reviewed evidence and leaves source messages intact",async()=>{
  const original=JSON.stringify(m.conv);
  expect(await upgradeDirectSession("source","continue goal","Reviewed progress","a")).toBe("linked");
  expect(m.plan).toHaveBeenCalledWith("linked","/repo","continue goal\n\nReviewed progress",expect.objectContaining({mode:"plan"}),false);
  expect(m.setChat).toHaveBeenCalledOnce();
  expect(JSON.stringify(m.conv)).toBe(original);
});
it("does not create a plan session or call a model when the workspace is dirty",async()=>{
  m.git.mockResolvedValue({isRepo:true,hasCommits:true,detached:false,changes:1});
  await expect(upgradeDirectSession("source","goal","brief","a")).rejects.toThrow("upgradeClean");
  expect(m.create).not.toHaveBeenCalled();expect(m.plan).not.toHaveBeenCalled();
});
it("does not upgrade unresolved execution or changed history",async()=>{
  m.history.mockResolvedValue([{finishedAt:null}]);
  await expect(upgradeDirectSession("source","goal","brief","a")).rejects.toThrow("unresolved");
  await expect(upgradeDirectSession("source","goal","brief","old")).rejects.toThrow("handoffStale");
  expect(m.plan).not.toHaveBeenCalled();
});
