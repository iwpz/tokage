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
