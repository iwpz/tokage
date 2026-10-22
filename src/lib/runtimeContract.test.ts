import {expect,it,vi} from "vitest";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn().mockResolvedValue({revision:1})}));
import {invoke} from "@tauri-apps/api/core";
import {runtimeSubmit,type RuntimeCommand} from "./runtime";
import fixture from "../../tests/fixtures/shared-runtime-v1.json";
for(const entry of fixture.commands)it(`App ${entry.app.type} matches the shared Rust/CLI wire contract`,async()=>{
 const request={schemaVersion:1 as const,runId:"contract",commandId:"command",expectedRevision:0,command:entry.app as RuntimeCommand};
 await runtimeSubmit(request);
 expect(invoke).toHaveBeenLastCalledWith("runtime_submit",{request:{...request,command:entry.wire}});
});
