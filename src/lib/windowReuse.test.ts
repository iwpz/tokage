import {expect,it,vi} from "vitest";
vi.mock("./ipc",()=>({evaluateWindowReuse:vi.fn(),inTauri:true}));
import {evaluateWindowReuse} from "./ipc";
import {findReusableWindow} from "./windowReuse";
it("a compatibility window without authoritative provider/usage inputs cannot authorize reuse with invented defaults",async()=>{
 const options={model:"unverified-model",prompt:"next task",task:{id:"T2",domain:"backend-api",write_scope:["src/**"]},pipeline:{tasks:[{id:"T1",domain:"backend-api",write_scope:["src/**"]}],taskStates:{T1:"DONE"},threadIds:{T1:"native-old"},taskEndedAt:{T1:Date.now()},upstreamArtifacts:{T1:{report:"previous work"}}}};
 expect(await findReusableWindow(options as unknown as Parameters<typeof findReusableWindow>[0])).toBeNull();expect(evaluateWindowReuse).not.toHaveBeenCalled();
});
