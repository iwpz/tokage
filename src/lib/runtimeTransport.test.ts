import {expect,it,vi} from "vitest";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn().mockResolvedValue({revision:7})}));
import {invoke} from "@tauri-apps/api/core";
import {runtimeSubmit} from "./runtime";
it("encodes the actual Rust schema-v1 command fields without rewriting nested effective configuration",async()=>{
 await runtimeSubmit({schemaVersion:1,commandId:"confirm",runId:"run",expectedRevision:6,command:{type:"ConfirmPlan",payload:{confirmationDigest:"digest",confirmedCommands:["cargo test"]}}});
 expect(invoke).toHaveBeenLastCalledWith("runtime_submit",{request:{schemaVersion:1,commandId:"confirm",runId:"run",expectedRevision:6,command:{type:"ConfirmPlan",payload:{confirmation_digest:"digest",confirmed_commands:["cargo test"]}}}});
 await runtimeSubmit({schemaVersion:1,commandId:"config",runId:"run",expectedRevision:7,command:{type:"UpdateConfig",payload:{config:{targetIdentity:"sha",permissionMode:"full-auto"}}}});
 expect(invoke).toHaveBeenLastCalledWith("runtime_submit",expect.objectContaining({request:expect.objectContaining({command:{type:"UpdateConfig",payload:{config:{targetIdentity:"sha",permissionMode:"full-auto"}}}})}));
});

it("preserves declared takeover duration in the public Rust command",async()=>{
 await runtimeSubmit({schemaVersion:1,commandId:"human",runId:"run",expectedRevision:7,command:{type:"AcceptTakeover",payload:{taskId:"T1",effortSeconds:90}}});
 expect(invoke).toHaveBeenLastCalledWith("runtime_submit",expect.objectContaining({request:expect.objectContaining({command:{type:"AcceptTakeover",payload:{task_id:"T1",effort_seconds:90}}})}));
});
it("encodes exact task context scope without changing nested preview contracts",async()=>{
 await runtimeSubmit({schemaVersion:1,commandId:"queue",runId:"run",expectedRevision:7,command:{type:"QueueTaskContext",payload:{taskId:"T1",sourceTurn:"a",confirmationDigest:"digest",configHash:"hash",recommendedAction:"FRESH"}}});
 expect(invoke).toHaveBeenLastCalledWith("runtime_submit",expect.objectContaining({request:expect.objectContaining({command:{type:"QueueTaskContext",payload:{task_id:"T1",source_turn:"a",confirmation_digest:"digest",config_hash:"hash",recommended_action:"FRESH"}}})}));
});
it("preserves explicit task routing and the reviewed target identity in schema-v1",async()=>{
 await runtimeSubmit({schemaVersion:1,commandId:"route",runId:"run",expectedRevision:7,command:{type:"AppendRoutedTaskMessage",payload:{message:" exact text\n ",explicitTaskId:"T1",targetTaskId:"T1",sourceTurn:"a",configHash:"hash",confirmationDigest:"digest"}}});
 expect(invoke).toHaveBeenLastCalledWith("runtime_submit",expect.objectContaining({request:expect.objectContaining({command:{type:"AppendRoutedTaskMessage",payload:{message:" exact text\n ",explicit_task_id:"T1",target_task_id:"T1",source_turn:"a",config_hash:"hash",confirmation_digest:"digest"}}})}));
});
