// @vitest-environment happy-dom
import {act} from "react";
import {createRoot} from "react-dom/client";
import {renderToString} from "react-dom/server";
import {it,expect} from "vitest";
import "../../i18n";
import type {ContextAuditAction} from "../../lib/ipc";
import {ContextAuditTrail} from "./ContextAuditTrail";
const row:ContextAuditAction={key:"run:b:REUSE",kind:"REUSE",runId:"run",taskId:"T2",attemptId:"b",sourceTurn:"a",sourceThread:"history-window",targetThread:"current-window",windowOrigin:"a",status:"FAILED_OR_FRESH_FALLBACK",reason:"thread changed",occurredAt:1,parameters:{sources:{price:"pack"}},receiptKey:"turn:b",reportedUsd:null,estimatedUsd:null,settlement:"COST_UNKNOWN",baselineUsd:1,forecastDeltaUsd:-1,measuredSavingUsd:null,evidenceDigest:"digest"};
it("keeps failed reuse, historical/current windows, signed forecasts and unknown fees separate",()=>{
 const html=renderToString(<ContextAuditTrail rows={[row]}/>).replace(/<!--.*?-->/g,"");
 expect(html).toContain("history-window");expect(html).toContain("current-window");expect(html).toContain("turn:b");expect(html).toContain("-$1.0000");expect(html).toContain("thread changed");expect(html).not.toContain("$0.00");expect(html).toContain("pack");
});
it("filters actual rendered rows by action and source/target IDs without mixing window history",async()=>{
 const host=document.createElement("div");document.body.append(host);const root=createRoot(host);
 await act(async()=>{root.render(<ContextAuditTrail rows={[row,{...row,key:"run:c:CONTEXT",kind:"CONTEXT",attemptId:"c",sourceThread:"other-window",targetThread:"new-window"}]}/>);});
 const select=host.querySelector("select")!;await act(async()=>{select.value="REUSE";select.dispatchEvent(new Event("change",{bubbles:true}));});
 expect(host.querySelectorAll("ol > li")).toHaveLength(1);expect(host.textContent).toContain("history-window");
 const input=host.querySelector("input")!;await act(async()=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!.call(input,"other-window");input.dispatchEvent(new Event("input",{bubbles:true}));});
 expect(host.querySelectorAll("ol > li")).toHaveLength(0);
 await act(async()=>{root.unmount();});host.remove();
});
