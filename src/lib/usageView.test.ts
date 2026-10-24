import { expect, it } from "vitest";
import type { AccountUsageResult, ProviderInfo } from "./ipc";
import { usageView } from "./usageView";
const p={id:"COPILOT_CLI",status:"READY",usageQuery:{state:"IDENTITY_REQUIRED",method:"CLI"}} as ProviderInfo;
const state={detecting:false,pending:false,active:false,enabled:true,consentReady:true,allowed:true};
it("never turns missing quota data or unverified history into a login error",()=>{
 expect(usageView(p,undefined,state)).toBe("notRequested");
 for(const status of ["NOT_REQUESTED","UNVERIFIED","IDENTITY_UNKNOWN","IDENTITY_CHANGED"] as const){
  expect(usageView(p,{status} as AccountUsageResult,state)).not.toBe("needsLogin");
 }
 expect(usageView({...p,status:"NEEDS_LOGIN"},undefined,state)).toBe("needsLogin");
});
it("separates queue, processing, disabled consent, and unsupported quota from authentication",()=>{
 expect(usageView(p,undefined,{...state,pending:true})).toBe("queued");
 expect(usageView(p,undefined,{...state,pending:true,active:true})).toBe("querying");
 expect(usageView(p,undefined,{...state,allowed:false})).toBe("offline");
 expect(usageView({...p,usageQuery:{...p.usageQuery!,state:"IDENTITY_PENDING"}},undefined,state)).toBe("unsupported");
 expect(usageView({...p,status:"NEEDS_LOGIN",usageQuery:{...p.usageQuery!,backend:"deepseek"}},undefined,state)).toBe("notRequested");
});
