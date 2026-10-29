// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach,expect,it,vi } from "vitest";
const calls=vi.hoisted(()=>({history:vi.fn()}));
vi.mock("../../lib/ipc",async original=>({...await original<object>(),listContextActionHistory:calls.history}));
vi.mock("react-i18next",async original=>({...await original<object>(),useTranslation:()=>({t:(key:string)=>key})}));
import { ContextActionHistory } from "./ContextActionHistory";
import { useChat,type Conversation } from "../../stores/chat";
const conversation:Conversation={thread:{id:"window",ref:{provider:"CODEX_CLI",model:"gpt"},mode:"direct"},messages:[{id:"source",role:"assistant",text:"",mode:"direct",nativeThreadSessionId:"s",nativeThreadId:"window",ref:{provider:"CODEX_CLI",model:"gpt"}}]};
beforeEach(()=>{vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT",true);vi.clearAllMocks();useChat.setState({conversations:{s:conversation}});});
async function open(host:HTMLDivElement) {
 await act(async()=>{const details=host.querySelector("details")!;details.open=true;details.dispatchEvent(new Event("toggle"));});
}
it("reads on demand and rejects late results after switching the native window",async()=>{
 let finish!:(rows:unknown[])=>void;
 calls.history.mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;})).mockResolvedValueOnce([{id:2,kind:"FRESH",status:"APPLIED",trigger:"USER",attemptId:"<source>",actualSavingUsd:0}]);
 const host=document.createElement("div"),root=createRoot(host);
 try {
  await act(async()=>root.render(<ContextActionHistory sessionId="s"/>));
  expect(calls.history).not.toHaveBeenCalled();
  await open(host);expect(calls.history).toHaveBeenCalledWith("s",["source"]);
  expect(host.textContent).toContain("workspace.context.historyLoading");
  await act(async()=>useChat.setState({conversations:{s:{...conversation,thread:{...conversation.thread!,id:"next-window"},messages:[...conversation.messages,{...conversation.messages[0],id:"next",nativeThreadId:"next-window"}]}}}));
  expect(calls.history).toHaveBeenLastCalledWith("s",["next"]);
  await act(async()=>finish([{id:1,kind:"COMPACT",status:"FAILED",trigger:"USER",attemptId:"old"}]));
  expect(host.textContent).toContain("#2");expect(host.textContent).not.toContain("#1");
  expect(host.innerHTML).toContain("&lt;source&gt;");expect(host.textContent).toContain("workspace.context.historyEstimate");
 }finally{await act(async()=>root.unmount());}
});
it("shows query failures and allows a read-only refresh to an empty result",async()=>{
 calls.history.mockRejectedValueOnce(new Error("Read unavailable")).mockResolvedValueOnce([]);
 const host=document.createElement("div"),root=createRoot(host);
 try {
  await act(async()=>root.render(<ContextActionHistory sessionId="s"/>));await open(host);
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("Read unavailable");
  await act(async()=>host.querySelector("button")!.click());
  expect(calls.history).toHaveBeenCalledTimes(2);expect(host.textContent).toContain("workspace.context.historyEmpty");
 }finally{await act(async()=>root.unmount());}
});
