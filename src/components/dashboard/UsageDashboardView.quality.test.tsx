// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
const api=vi.hoisted(()=>({read:vi.fn()}));
vi.mock("../../lib/ipc",async original=>({...await original<object>(),getUsageDashboard:api.read}));
import { UsageDashboardView } from "./UsageDashboardView";
const fixture=(range="7d",rate:number|null=null)=>({range,providers:[],quality:{firstPassRate:rate,samples:rate===null?0:2,legacyUnverifiedSamples:5,unknownTimeSamples:1,matrix:[],observations:[]},savings:{totalUsd:0,byKind:{},referenceModel:""},costSplit:{planningUsd:0,executionUsd:0,repairUsd:0},tokenSeries:[],dailyUsage:[]});
let host:HTMLDivElement,root:Root;
beforeEach(()=>{vi.useFakeTimers();(globalThis as any).IS_REACT_ACT_ENVIRONMENT=true;api.read.mockReset();host=document.createElement("div");document.body.append(host);root=createRoot(host);});
afterEach(()=>{act(()=>root.unmount());host.remove();vi.useRealTimers();});
const render=()=>act(async()=>{root.render(createElement(UsageDashboardView));await vi.advanceTimersByTimeAsync(0);});
const click=(text:string)=>act(async()=>{[...host.querySelectorAll<HTMLButtonElement>("button")].find(n=>n.textContent?.trim()===text)!.click();await vi.advanceTimersByTimeAsync(0);});
it("shows no measured rate for empty samples and explains excluded historical rows",async()=>{
 api.read.mockResolvedValue(fixture());await render();
 const title=[...host.querySelectorAll("span")].find(n=>n.textContent==="First Pass Rate")!;
 const card=title.closest(".p-5")!;
 expect(card.textContent).toContain("—");expect(card.textContent).not.toContain("100%");
 expect(card.textContent).toContain("0 verified task observations");expect(card.textContent).toContain("5 legacy rows");expect(card.textContent).toContain("predictions are not measured guarantees");
});
it("does not publish a late prior-period response over the selected period",async()=>{
 let resolve:(value:unknown)=>void=()=>{};
 api.read.mockImplementationOnce(()=>new Promise(r=>{resolve=r;})).mockResolvedValue(fixture("today",0.5));
 await render();await click("Today");
 expect(host.textContent).toContain("50%");
 await act(async()=>resolve(fixture("7d",1)));
 expect(host.textContent).toContain("50%");expect(host.textContent).not.toContain("100%");
});
it("clears stale quality on failure rather than claiming a current successful rate",async()=>{
 api.read.mockResolvedValueOnce(fixture("7d",1)).mockRejectedValue(new Error("corrupt runtime"));
 await render();expect(host.textContent).toContain("100%");await click("Today");
 expect(host.querySelector('[role="alert"]')?.textContent).toContain("Quality data unavailable");
 expect(host.textContent).not.toContain("100%");
});

it("does not trust a legacy backend percentage without verified sample provenance",async()=>{
 const legacy:any=fixture("7d",1);delete legacy.quality.samples;
 api.read.mockResolvedValue(legacy);await render();
 expect(host.textContent).not.toContain("100%");
});
it("exposes takeover provenance separately from final delivery and excludes undated records",async()=>{
 const data:any=fixture();data.quality.unknownTimeSamples=1;
 data.quality.observations=[{key:"stable-task",runId:"run-one",taskId:"T1",revision:42,runState:"AwaitingMerge",provider:"CODEX_CLI",model:"requested-model",modelBasis:"REQUESTED",outcome:"USER_TAKEOVER",learningEligible:false,attemptIds:["failed-attempt","human-attempt"],observedAt:null,evidenceDigest:"evidence-sha"}];
 api.read.mockResolvedValue(data);await render();
 expect(host.querySelector("details")?.textContent).toContain("Accepted after human takeover");
 expect(host.textContent).toContain("AwaitingMerge");expect(host.textContent).toContain("run-one");
 expect(host.textContent).toContain("failed-attempt, human-attempt");expect(host.textContent).toContain("evidence-sha");
 expect(host.textContent).toContain("Date unknown; excluded from this period rate.");
 expect(host.textContent).not.toContain("100%");
});
