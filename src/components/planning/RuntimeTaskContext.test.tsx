// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, it, expect, vi } from "vitest";
vi.mock("../../lib/runtime",async original=>({...await original<object>(),runtimeTaskContext:vi.fn()}));
import {runtimeTaskContext} from "../../lib/runtime";
import {RuntimeTaskContext} from "./RuntimeTaskContext";
import i18n from "../../i18n";
import {taskContextInputs,taskContextSnapshot,taskContextPreparation} from "../../../tests/fixtures/runtime-context";
let host:HTMLDivElement,root:Root;
const command=vi.fn();
beforeEach(async()=>{(globalThis as any).IS_REACT_ACT_ENVIRONMENT=true;vi.clearAllMocks();await i18n.changeLanguage("en");vi.mocked(runtimeTaskContext).mockResolvedValue(taskContextInputs);host=document.createElement("div");document.body.append(host);root=createRoot(host);});
afterEach(()=>{act(()=>root.unmount());host.remove();});
async function render(s=taskContextSnapshot(),busy=false){await act(async()=>{root.render(<RuntimeTaskContext snapshot={s} taskId="T1" busy={busy} onCommand={command}/>);});}
const button=(text:string)=>[...host.querySelectorAll("button")].find(b=>b.textContent===text)!;
it("reads authoritative preview without dispatch, then queues only the selected source and frozen approval",async()=>{
 await render();expect(runtimeTaskContext).toHaveBeenCalledWith("run","T1","a");expect(command).not.toHaveBeenCalled();
 expect(host.textContent).toContain("Suggested action: FRESH");
 await act(async()=>button("Queue this action").click());
 expect(command).toHaveBeenCalledExactlyOnceWith({type:"QueueTaskContext",payload:{taskId:"T1",sourceTurn:"a",confirmationDigest:"digest",configHash:"hash",recommendedAction:"FRESH"}});
});
it("undo is source-scoped and completed carrier never becomes native confirmation",async()=>{
 const s=taskContextSnapshot();s.execution!.tasks.T1.contextActions=[taskContextPreparation];
 await render(s);await act(async()=>button("Undo queued action").click());
 expect(command).toHaveBeenLastCalledWith({type:"CancelTaskContext",payload:{taskId:"T1",sourceTurn:"a"}});
 s.execution!.tasks.T1.contextActions=[];
 s.turns!.a.contextPreparation={...taskContextPreparation,targetTurn:"a",status:"BOUND",boundAt:1};
 await render(s);expect(host.textContent).toContain("Bound to send");expect(host.textContent).toContain("Carrier turn: completed");expect(host.textContent).toContain("Native compaction: unconfirmed");expect(host.textContent).not.toContain("Native compaction confirmed");
});
it("stale preview response cannot reappear after switching task window",async()=>{
 let finish!:(v:typeof taskContextInputs)=>void;
 vi.mocked(runtimeTaskContext).mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;}));
 const s=taskContextSnapshot();await render(s);
 s.execution!.tasks.T1.attempts=[];s.turns={};s.revision++;
 await render(s);await act(async()=>finish(taskContextInputs));
 expect(host.textContent).not.toContain("Suggested action: FRESH");expect(button("Queue this action").disabled).toBe(true);expect(command).not.toHaveBeenCalled();
});
it("off, stale reads, cooldown, unknown history and terminal tasks cannot queue",async()=>{
 const s=taskContextSnapshot();await render(s,true);expect(button("Queue this action").disabled).toBe(true);
 for(const inputs of [{...taskContextInputs,cooldown:{...taskContextInputs.cooldown,known:false}},{...taskContextInputs,preview:{...taskContextInputs.preview,cooldownBlocked:true}}]){
  vi.mocked(runtimeTaskContext).mockResolvedValue(inputs);s.revision++;await render(s);expect(button("Queue this action").disabled).toBe(true);
 }
 s.config.contextPolicy="off";s.revision++;await render(s);expect(host.textContent).toContain("Context actions are disabled");expect(button("Queue this action")).toBeUndefined();
 s.config.contextPolicy="auto";s.execution!.tasks.T1.state="DONE";s.revision++;await render(s);expect(button("Queue this action").disabled).toBe(true);
 expect(command).not.toHaveBeenCalled();
});
it("shows failures and queued automatic work separately",async()=>{
 const s=taskContextSnapshot();s.state="Executing";
 s.turns!.b={...s.turns!.a,id:"b",ended:false,status:"Starting",contextPreparation:{...taskContextPreparation,policy:"auto",userAccepted:false,targetTurn:"b"}};
 s.execution!.tasks.T1.attempts.push("b");await render(s);
 expect(host.textContent).toContain("Queued / prepared");expect(host.textContent).toContain("Carrier turn: pending");
 await act(async()=>button("Stop run before binding").click());expect(command).toHaveBeenLastCalledWith({type:"CancelRun"});
 s.turns!.b.contextPreparation!.status="FAILED";s.turns!.b.error="Fresh window was not confirmed";s.turns!.b.ended=true;s.turns!.b.status="Failed";
 await render(s);expect(host.textContent).toContain("Action failed");expect(host.textContent).toContain("Fresh window was not confirmed");expect(host.textContent).toContain("Carrier turn: failed / stopped");
});

it("keeps a failed queue receipt visible even when no carrier was launched",async()=>{
 const s=taskContextSnapshot();s.execution!.tasks.T1.contextActions=[{...taskContextPreparation,status:"FAILED",failure:"Run stopped before context binding: source changed"}];
 await render(s);expect(host.textContent).toContain("Action failed");expect(host.textContent).toContain("source changed");expect(host.textContent).not.toContain("Carrier turn: completed");expect(host.textContent).toContain("Native compaction: unconfirmed");
});
