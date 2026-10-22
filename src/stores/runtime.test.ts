import { beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("../lib/runtime",()=>({runtimeSnapshot:vi.fn(),runtimeWorkerError:vi.fn().mockResolvedValue(null),runtimeEvents:vi.fn(),runtimeTranscript:vi.fn(),runtimeSubmit:vi.fn(),runtimeRequest:vi.fn((s,c)=>({runId:s.runId,expectedRevision:s.revision,command:c}))}));
import { runtimeSnapshot,runtimeEvents,runtimeTranscript,runtimeSubmit,type RuntimeSnapshot } from "../lib/runtime";
import { observeRuntime,useRuntime } from "./runtime";
const snapshot=(revision=1):RuntimeSnapshot=>({schemaVersion:1,runId:"run",revision,state:"Draft",requirement:"feature",config:{} as RuntimeSnapshot["config"],configHash:"hash"});
beforeEach(()=>{vi.clearAllMocks();useRuntime.setState({runs:{},errors:{}});vi.mocked(runtimeSnapshot).mockResolvedValue(snapshot());vi.mocked(runtimeEvents).mockResolvedValue([{schemaVersion:1,runId:"run",sequence:1,revision:1,turnId:null,attemptId:null,kind:"RunCreated"}]);vi.mocked(runtimeTranscript).mockResolvedValue([]);});
describe("runtime observation",()=>{
 it("deduplicates events and never dispatches on refresh or reconnect",async()=>{
  await useRuntime.getState().refresh("run");await useRuntime.getState().refresh("run");
  expect(useRuntime.getState().runs.run.cursor).toBe(1);expect(runtimeEvents).toHaveBeenLastCalledWith("run",1);expect(runtimeSubmit).not.toHaveBeenCalled();
 });
 it("rejects gaps without advancing cursor or replacing the projection",async()=>{
  await useRuntime.getState().refresh("run");vi.mocked(runtimeSnapshot).mockResolvedValue(snapshot(3));vi.mocked(runtimeEvents).mockResolvedValue([{schemaVersion:1,runId:"run",sequence:3,revision:3,turnId:null,attemptId:null,kind:"TurnObserved"}]);
  await expect(useRuntime.getState().refresh("run")).rejects.toThrow("gap");expect(useRuntime.getState().runs.run.cursor).toBe(1);expect(useRuntime.getState().runs.run.snapshot.revision).toBe(1);expect(runtimeSubmit).not.toHaveBeenCalled();
 });
 it("destroying a view never cancels or resumes work",async()=>{
  const dispose=observeRuntime("run");dispose();await useRuntime.getState().refresh("run");expect(runtimeSubmit).not.toHaveBeenCalled();
 });
 it("sends only an explicit intent against the current durable revision",async()=>{
  vi.mocked(runtimeSnapshot).mockResolvedValue(snapshot(7));await useRuntime.getState().command("run",{type:"CancelRun"});expect(runtimeSubmit).toHaveBeenCalledExactlyOnceWith({runId:"run",expectedRevision:7,command:{type:"CancelRun"}});
 });
});
it("reconstructs durable transcript after discarding the view projection without dispatch",async()=>{
 const s=snapshot(3);s.turns={turn:{id:"turn",lastSequence:2} as NonNullable<RuntimeSnapshot["turns"]>[string]};vi.mocked(runtimeSnapshot).mockResolvedValue(s);
 vi.mocked(runtimeTranscript).mockResolvedValue([{turnId:"turn",sequence:1,event:{kind:"text",text:"first"}},{turnId:"turn",sequence:2,event:{kind:"text",text:"second"}}]);
 await useRuntime.getState().refresh("run");const original=useRuntime.getState().runs.run.transcripts;
 useRuntime.setState({runs:{}});await useRuntime.getState().refresh("run");
 expect(useRuntime.getState().runs.run.transcripts).toEqual(original);expect(runtimeTranscript).toHaveBeenLastCalledWith("run","turn",0);expect(runtimeSubmit).not.toHaveBeenCalled();
});

it("fences mutations against the revision actually displayed instead of silently rebasing stale edits",async()=>{
 await useRuntime.getState().refresh("run");vi.mocked(runtimeSnapshot).mockResolvedValue(snapshot(7));
 await useRuntime.getState().command("run",{type:"UpdateConfig",payload:{config:{source:"user edit"}}});
 expect(runtimeSubmit).toHaveBeenCalledExactlyOnceWith({runId:"run",expectedRevision:1,command:{type:"UpdateConfig",payload:{config:{source:"user edit"}}}});
});
it("updates sidebar status only as a projection of the authoritative run",async()=>{
 const {useProjects}=await import("./projects");useProjects.setState({projects:[{id:"p",name:"Project",path:"/repo",expanded:true,sessions:[{id:"run",title:"Run",state:"draft",updatedAt:1}]}]});
 const s=snapshot();s.state="Executing";vi.mocked(runtimeSnapshot).mockResolvedValue(s);await useRuntime.getState().refresh("run");
 expect(useProjects.getState().projects[0].sessions[0].state).toBe("running");expect(useRuntime.getState().runs.run.snapshot.state).toBe("Executing");expect(runtimeSubmit).not.toHaveBeenCalled();
});
