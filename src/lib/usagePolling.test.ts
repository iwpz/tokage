// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
const f=vi.hoisted(()=>({ permission:{ready:true,busy:false,allowOnlineUsageQuery:false}, store:{scanning:false,pending:[] as string[],usagePending:[] as string[],realProviders:[{id:"OPENCODE",status:"READY",usageQuery:{backend:"deepseek",state:"IDENTITY_REQUIRED",method:"HTTP"}}],usageResults:{},refreshQuota:vi.fn(),refresh:vi.fn()},settings:{providerEnabled:{}},listeners:[] as (()=>void)[] }));
vi.mock("../stores/onlineUsage",()=>({useOnlineUsage:{getState:()=>f.permission,subscribe:(cb:()=>void)=>{f.listeners.push(cb);return vi.fn();}}}));
vi.mock("../stores/providers",()=>({useProviders:{getState:()=>f.store,subscribe:()=>vi.fn()}}));
vi.mock("../stores/settings",()=>({useSettings:{getState:()=>f.settings,subscribe:()=>vi.fn()}}));
import { startUsagePolling } from "./usagePolling";
beforeEach(()=>{vi.useFakeTimers();vi.setSystemTime(1_000_000);f.store.refreshQuota.mockReset();f.permission.allowOnlineUsageQuery=false;f.listeners=[];});
it("starts on consent, deduplicates immediate events, and cleans up on unmount",()=>{
 const stop=startUsagePolling();expect(f.store.refreshQuota).not.toHaveBeenCalled();
 f.permission.allowOnlineUsageQuery=true;f.listeners[0]();expect(f.store.refreshQuota).toHaveBeenCalledWith("OPENCODE",true);
 f.listeners[0]();expect(f.store.refreshQuota).toHaveBeenCalledTimes(1);
 stop();vi.advanceTimersByTime(120_000);expect(f.store.refreshQuota).toHaveBeenCalledTimes(1);vi.useRealTimers();
});
it("never queries while detection is running or native consent is busy",()=>{
 f.permission.allowOnlineUsageQuery=true;f.permission.busy=true;
 const stop=startUsagePolling();expect(f.store.refreshQuota).not.toHaveBeenCalled();
 f.permission.busy=false;f.store.scanning=true;f.listeners[0]();expect(f.store.refreshQuota).not.toHaveBeenCalled();
 f.store.scanning=false;f.listeners[0]();expect(f.store.refreshQuota).toHaveBeenCalledTimes(1);
 stop();vi.useRealTimers();
});
