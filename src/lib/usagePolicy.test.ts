import { expect, it } from "vitest";
import type { AccountUsageResult, ProviderInfo } from "./ipc";
import { canQueryUsage, usageDue } from "./usagePolicy";
it("requires a supported source and separates named wallets from session login", () => {
  const p={status:"READY",usageQuery:{state:"IDENTITY_REQUIRED",method:"CLI",authentication:"OFFICIAL_SUBSCRIPTION"},account:{official:true,billing:"SUBSCRIPTION"}} as ProviderInfo;
  expect(canQueryUsage(p)).toBe(true);
  expect(canQueryUsage({...p,status:"NEEDS_LOGIN"})).toBe(false);
  expect(canQueryUsage({...p,usageQuery:{...p.usageQuery!,state:"IDENTITY_PENDING"}})).toBe(false);
  expect(canQueryUsage({...p,status:"NEEDS_LOGIN",usageQuery:{...p.usageQuery!,backend:"deepseek"},account:{...p.account!,official:false}})).toBe(true);
});
it("respects TTL, authentication blocks, retry-after and attempt spacing", () => {
  const now=1_000_000;
  expect(usageDue(undefined,now-1000,now)).toBe(false);
  expect(usageDue(undefined,0,now)).toBe(true);
  const r={status:"LATEST",latestSuccess:{validUntil:1100},attemptedAt:900,error:null,nextRetryAt:null} as AccountUsageResult;
  expect(usageDue(r,0,now)).toBe(false);
  expect(usageDue({...r,status:"FAILED",nextRetryAt:1200},0,now)).toBe(false);
  expect(usageDue({...r,status:"FAILED",error:"AUTHENTICATION"},0,now)).toBe(false);
  expect(usageDue({...r,status:"IDENTITY_UNKNOWN",attemptedAt:990},0,now)).toBe(false);
  expect(usageDue({...r,status:"EXPIRED",latestSuccess:{...r.latestSuccess!,validUntil:999}},0,now)).toBe(true);
});
