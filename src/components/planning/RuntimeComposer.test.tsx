// @vitest-environment happy-dom
import {act} from "react";
import {createRoot,type Root} from "react-dom/client";
import {beforeEach,afterEach,it,expect,vi} from "vitest";
const mocks=vi.hoisted(()=>({state:{} as any,route:vi.fn(),submit:vi.fn(),refresh:vi.fn(),command:vi.fn(),trusted:true}));
vi.mock("../../stores/runtime",()=>({useRuntime:Object.assign((select:(state:any)=>unknown)=>select(mocks.state),{getState:()=>mocks.state})}));
vi.mock("../../stores/projects",()=>({useProjects:{getState:()=>({projects:[{trusted:mocks.trusted,sessions:[{id:"run"}]}]})}}));
vi.mock("../../lib/runtime",async original=>({...await original<object>(),runtimeRouteTaskMessage:mocks.route,runtimeSubmit:mocks.submit,runtimeTaskContext:vi.fn().mockResolvedValue(null)}));
import {RuntimeComposer} from "./RuntimeComposer";
import {taskContextSnapshot} from "../../../tests/fixtures/runtime-context";
import i18n from "../../i18n";
let host:HTMLDivElement,root:Root;
const preview={revision:7,configHash:"hash",planDigest:"digest",route:{type:"TASK_EXECUTION_WINDOW",task_id:"T1",reason:"Matched task T1"},taskId:"T1",sourceTurn:"a",unavailableReason:null};
beforeEach(async()=>{
 (globalThis as any).IS_REACT_ACT_ENVIRONMENT=true;vi.useFakeTimers();vi.clearAllMocks();mocks.trusted=true;
 await i18n.changeLanguage("en");mocks.route.mockResolvedValue(preview);mocks.submit.mockResolvedValue({revision:8});mocks.refresh.mockResolvedValue(undefined);
 mocks.state={runs:{run:{snapshot:taskContextSnapshot()}},readErrors:{},errors:{},command:mocks.command,refresh:mocks.refresh};
 host=document.createElement("div");document.body.append(host);root=createRoot(host);
});
afterEach(()=>{act(()=>root.unmount());host.remove();vi.useRealTimers();});
async function render(){await act(async()=>root.render(<RuntimeComposer sessionId="run"/>));}
async function type(text:string){await act(async()=>{const input=host.querySelector("textarea")!;Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,"value")!.set!.call(input,text);input.dispatchEvent(new Event("input",{bubbles:true}));input.dispatchEvent(new Event("change",{bubbles:true}));});await act(async()=>{await vi.advanceTimersByTimeAsync(210);});}
const send=()=>[...host.querySelectorAll("button")].find(b=>b.textContent==="Send to task")!;
it("previews auto routing before context and sends the exact message with the reviewed source and revision",async()=>{
 await render();expect(mocks.route).not.toHaveBeenCalled();await type("  T1: repair this\n ");
 expect(mocks.route).toHaveBeenLastCalledWith("run","  T1: repair this\n ",null);expect(mocks.submit).not.toHaveBeenCalled();expect(send().disabled).toBe(false);
 await act(async()=>{send().click();send().click();});
 expect(mocks.submit).toHaveBeenCalledTimes(1);expect(mocks.submit).toHaveBeenCalledWith(expect.objectContaining({runId:"run",expectedRevision:7,command:{type:"AppendRoutedTaskMessage",payload:{message:"  T1: repair this\n ",explicitTaskId:null,targetTaskId:"T1",sourceTurn:"a",configHash:"hash",confirmationDigest:"digest"}}}));
 expect(host.querySelector("textarea")!.value).toBe("");
});
it("explicit selection is passed to routing and an unavailable target is retained instead of selecting a neighbour",async()=>{
 await render();await act(async()=>{const select=host.querySelector("select")!;select.value="T1";select.dispatchEvent(new Event("change",{bubbles:true}));});await type(" keep user text ");
 expect(mocks.route).toHaveBeenLastCalledWith("run"," keep user text ","T1");
 const s=mocks.state.runs.run.snapshot;s.plan.plan.tasks=[];s.revision++;mocks.route.mockRejectedValue(new Error("Explicit target task no longer exists"));await render();await act(async()=>{await vi.advanceTimersByTimeAsync(210);});
 expect(host.querySelector("select")!.value).toBe("T1");expect(host.textContent).toContain("Unavailable target: T1");expect(send().disabled).toBe(true);expect(host.querySelector("textarea")!.value).toBe(" keep user text ");expect(mocks.submit).not.toHaveBeenCalled();
});
it("terminal or active tasks, unapproved plans and stale reads cannot send",async()=>{
 await render();for(const reason of ["Task is finished","Pending approval","Native cleanup unknown"]){mocks.route.mockResolvedValue({...preview,unavailableReason:reason});await type(reason);expect(send().disabled).toBe(true);expect(host.textContent).toContain(reason);}
 mocks.route.mockResolvedValue(preview);await type("T1");mocks.state.readErrors.run="Read failed";await render();expect(send().disabled).toBe(true);expect(mocks.submit).not.toHaveBeenCalled();
});
it("late routing cannot replace a newer explicit target preview",async()=>{
 let finish!:(v:typeof preview)=>void;mocks.route.mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;}));await render();await type("old message");
 mocks.route.mockResolvedValue({...preview,taskId:null,sourceTurn:null,route:{type:"DIRECT_DISPATCH",reason:"standalone"}});await type("new message");await act(async()=>finish(preview));
 expect(host.textContent).toContain("standalone");expect(send().disabled).toBe(true);expect(mocks.submit).not.toHaveBeenCalled();
});
it("a rejected target or lost submit acknowledgement keeps the draft and never dispatches through a fallback",async()=>{
 await render();await type("T1 follow-up");mocks.submit.mockRejectedValue(new Error("revision conflict"));await act(async()=>send().click());
 expect(host.querySelector("textarea")!.value).toBe("T1 follow-up");expect(host.textContent).toContain("revision conflict");expect(mocks.submit).toHaveBeenCalledTimes(1);expect(mocks.command).not.toHaveBeenCalled();
});
it("trust is checked at send time and plan/direct routes remain explanatory",async()=>{
 await render();await type("T1");mocks.trusted=false;await act(async()=>send().click());expect(mocks.submit).not.toHaveBeenCalled();
 mocks.route.mockResolvedValue({...preview,taskId:null,sourceTurn:null,route:{type:"INCREMENTAL_REPLAN",reason:"changed scope"}});await type("replan");expect(host.textContent).toContain("Review and amend");expect(send().disabled).toBe(true);
});
