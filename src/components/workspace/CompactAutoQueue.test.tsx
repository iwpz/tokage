// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vitest";
import { CompactNextTurn } from "./CompactNextTurn";
import { useChat } from "../../stores/chat";
import { useSettings } from "../../stores/settings";
import type { ResolvedContextInputs } from "../../lib/contextInputs";
vi.mock("react-i18next",async original=>({...await original<object>(),useTranslation:()=>({t:(key:string)=>key})}));
const queue=vi.fn(), cancel=vi.fn();
const inputs={cooldown:{known:true},preview:{recommendedAction:"COMPACT",cooldownBlocked:false,cooldownRemainingTurns:0},compactIntent:{scope:{turnId:"a"},nextTurn:{type:"FRESH_WINDOW_BRIEF",brief:"retained"}}} as ResolvedContextInputs;
beforeEach(()=>{
 vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT",true);vi.clearAllMocks();
 queue.mockResolvedValue(undefined);cancel.mockResolvedValue(undefined);
 useChat.setState({conversations:{},queueCompact:queue,cancelCompact:cancel});useSettings.setState({contextMode:"auto"});
});
it("queues once on auto preview, preserves undo suppression, and allows a new completed turn",async()=>{
 const host=document.createElement("div"),root=createRoot(host);
 try {
  await act(async()=>root.render(<CompactNextTurn sessionId="s" inputs={inputs}/>));
  expect(queue).toHaveBeenCalledWith("s",inputs.compactIntent,true);
  await act(async()=>root.render(<CompactNextTurn sessionId="s" inputs={{...inputs}}/>));
  expect(queue).toHaveBeenCalledTimes(1);
  await act(async()=>useChat.setState({conversations:{s:{messages:[],thread:null,autoCompactDismissedTurnId:"a"}}}));
  expect(queue).toHaveBeenCalledTimes(1);
  const next={...inputs,compactIntent:{...inputs.compactIntent!,scope:{...inputs.compactIntent!.scope,turnId:"next"}}};
  await act(async()=>root.render(<CompactNextTurn sessionId="s" inputs={next}/>));
  expect(queue).toHaveBeenCalledTimes(2);
 } finally {await act(async()=>root.unmount());}
});
it("undoes automatic pending actions on mode disable and does not apply a suggest preview",async()=>{
 const host=document.createElement("div"),root=createRoot(host);
 try {
  useSettings.setState({contextMode:"suggest"});
  await act(async()=>root.render(<CompactNextTurn sessionId="s" inputs={inputs}/>));
  expect(queue).not.toHaveBeenCalled();
  await act(async()=>useChat.setState({conversations:{s:{messages:[],thread:null,pendingCompact:{intent:inputs.compactIntent!,requestKey:"key",automatic:true,actionId:1}}}}));
  expect(cancel).toHaveBeenCalledWith("s");
 } finally {await act(async()=>root.unmount());}
});
it("does not queue blocked, missing-history, native or FRESH-only previews",async()=>{
 const host=document.createElement("div"),root=createRoot(host);
 try {
  for(const value of [
   {...inputs,automaticQueueSuppressed:true},
   {...inputs,preview:{...inputs.preview,cooldownBlocked:true}},
   {...inputs,cooldown:{...inputs.cooldown,known:false}},
   {...inputs,compactIntent:{...inputs.compactIntent!,nextTurn:{type:"NATIVE_COMMAND",command:"/compact",transportReady:false}}},
   {...inputs,preview:{...inputs.preview,recommendedAction:"FRESH"}},
  ] as ResolvedContextInputs[]) await act(async()=>root.render(<CompactNextTurn sessionId="s" inputs={value}/>));
  expect(queue).not.toHaveBeenCalled();
 } finally {await act(async()=>root.unmount());}
});
