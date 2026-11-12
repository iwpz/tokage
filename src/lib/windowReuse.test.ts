import {expect,it,vi} from "vitest";
import {invoke} from "@tauri-apps/api/core";
import {findReusableWindow} from "./windowReuse";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));
it("a compatibility window without authoritative provider/usage inputs cannot authorize reuse with invented defaults",async()=>{
 const options={model:"unverified-model",prompt:"next task",task:{id:"T2",domain:"backend-api",write_scope:["src/**"]},pipeline:{tasks:[{id:"T1",domain:"backend-api",write_scope:["src/**"]}],taskStates:{T1:"DONE"},threadIds:{T1:"native-old"},taskEndedAt:{T1:Date.now()},upstreamArtifacts:{T1:{report:"previous work"}}}};
 expect(await findReusableWindow(options as unknown as Parameters<typeof findReusableWindow>[0])).toBeNull();expect(invoke).not.toHaveBeenCalled();
});
