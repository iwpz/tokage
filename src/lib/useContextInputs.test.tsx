// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { useContextInputs } from "./useContextInputs";
import type { ContextInputRequest, ResolvedContextInputs } from "./contextInputs";
const mocks=vi.hoisted(()=>({invoke:vi.fn()}));
vi.mock("@tauri-apps/api/core",()=>({invoke:mocks.invoke}));
vi.mock("./ipc",()=>({inTauri:true,resolveContextInputs:(request:unknown)=>mocks.invoke("resolve_context_inputs_cmd",{request})}));
vi.mock("../stores/knowledge",()=>({useKnowledge:(select:(s:unknown)=>unknown)=>select({customPacks:[],modelOverrides:{}})}));
it("rejects a late previous-window response and issues no command while disabled",async()=>{
 vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT",true);
 const pending:((value:ResolvedContextInputs)=>void)[]=[];
 mocks.invoke.mockImplementation(()=>new Promise(resolve=>pending.push(resolve)));
 const request:ContextInputRequest={provider:"CLAUDE_CODE",model:"sonnet",window:"one",observations:[],cacheAware:true,minSaving:.15};
 function View({r,enabled=true}:{r:ContextInputRequest;enabled?:boolean}) { const value=useContextInputs(r,enabled);return <span>{value?.modelId ?? "unknown"}</span>; }
 const host=document.createElement("div"),root=createRoot(host);
 try {
  await act(async()=>root.render(<View r={request}/>));
  await act(async()=>root.render(<View r={{...request,window:"two"}}/>));
  await act(async()=>pending[0]({modelId:"old"} as ResolvedContextInputs));
  expect(host.textContent).toBe("unknown");
  await act(async()=>pending[1]({modelId:"new"} as ResolvedContextInputs));
  expect(host.textContent).toBe("new");
  await act(async()=>root.render(<View r={request} enabled={false}/>));
  expect(mocks.invoke).toHaveBeenCalledTimes(2);expect(host.textContent).toBe("unknown");
 } finally {await act(async()=>root.unmount());vi.unstubAllGlobals();}
});
