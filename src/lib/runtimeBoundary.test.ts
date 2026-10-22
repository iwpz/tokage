import {beforeEach,expect,it,vi} from "vitest";
const mocks=vi.hoisted(()=>({snapshot:vi.fn(),command:vi.fn()}));
vi.mock("./ipc",async()=>({...await vi.importActual<typeof import("./ipc")>("./ipc"),inTauri:true}));
vi.mock("./runtime",()=>({runtimeSnapshot:mocks.snapshot}));
vi.mock("../stores/runtime",()=>({useRuntime:{getState:()=>({command:mocks.command})}}));
import {dispatchReadyTasks,ensureSessionIntegration,interruptExecution,continueTask} from "./taskRunner";
beforeEach(()=>{vi.clearAllMocks();mocks.snapshot.mockResolvedValue({runId:"shared",revision:7,state:"Executing"});});
it("cannot dispatch or integrate shared runs through the retired frontend engine",async()=>{
 await expect(dispatchReadyTasks("shared")).rejects.toThrow("owned by runtime");await expect(ensureSessionIntegration("shared")).rejects.toThrow("owned by runtime");expect(mocks.command).not.toHaveBeenCalled();
});
it("routes explicit cancel and retry intents to the shared authority",async()=>{
 await interruptExecution("shared");await continueTask("shared","T1");expect(mocks.command).toHaveBeenNthCalledWith(1,"shared",{type:"CancelRun"});expect(mocks.command).toHaveBeenNthCalledWith(2,"shared",{type:"RetryTask",payload:{taskId:"T1"}});
});
it("does not fall back to the old engine when the runtime query fails",async()=>{
 mocks.snapshot.mockRejectedValue(new Error("corrupt database"));await expect(dispatchReadyTasks("shared")).rejects.toThrow("corrupt database");expect(mocks.command).not.toHaveBeenCalled();
});
