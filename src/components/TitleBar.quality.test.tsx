// @vitest-environment happy-dom
import { beforeEach, afterEach, it, expect, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
const api=vi.hoisted(()=>({read:vi.fn()}));
vi.mock("../lib/ipc",async original=>({...await original<object>(),getUsageDashboard:api.read}));
vi.mock("./ProviderStatusMenu",()=>({ProviderStatusMenu:()=>null}));
vi.mock("../lib/layout",()=>({useLayout:()=>({docked:false})}));
import { TitleBar } from "./TitleBar";
let host:HTMLDivElement,root:Root;
beforeEach(()=>{vi.useFakeTimers();api.read.mockReset();(globalThis as any).IS_REACT_ACT_ENVIRONMENT=true;host=document.createElement("div");document.body.append(host);root=createRoot(host);});
afterEach(()=>{act(()=>root.unmount());host.remove();vi.useRealTimers();});
it("shows an unknown first-pass label instead of 100 percent for an empty sample",async()=>{
 api.read.mockResolvedValue({quality:{firstPassRate:null,samples:0},savings:{totalUsd:0}});
 await act(async()=>{root.render(createElement(TitleBar));await vi.advanceTimersByTimeAsync(0);});
 expect(host.textContent).toContain("no verified observations");expect(host.textContent).not.toContain("100%");
});
it("does not retain a stale success pill after the next quality read fails",async()=>{
 api.read.mockResolvedValueOnce({quality:{firstPassRate:1,samples:1},savings:{totalUsd:0}}).mockRejectedValue(new Error("read failed"));
 await act(async()=>{root.render(createElement(TitleBar));await vi.advanceTimersByTimeAsync(0);});
 expect(host.textContent).toContain("100%");
 await act(async()=>{await vi.advanceTimersByTimeAsync(15000);});
 expect(host.textContent).not.toContain("100%");
});
