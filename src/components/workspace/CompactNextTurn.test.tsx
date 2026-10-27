import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import { CompactNextTurn } from "./CompactNextTurn";
import { useChat } from "../../stores/chat";
import type { ResolvedContextInputs } from "../../lib/contextInputs";
vi.mock("react-i18next",async importOriginal=>({...await importOriginal<object>(),useTranslation:()=>({t:(key:string)=>key})}));
it("shows native path unavailable without an apply control",()=>{
 useChat.setState({conversations:{}});
 const inputs={cooldown:{known:true},preview:{recommendedAction:"COMPACT",cooldownBlocked:false,cooldownRemainingTurns:0},compactIntent:{nextTurn:{type:"NATIVE_COMMAND",transportReady:false,command:"/compact"}}} as ResolvedContextInputs;
 const html=renderToStaticMarkup(<CompactNextTurn sessionId="s" inputs={inputs}/>);
 expect(html).toContain("workspace.context.nativeDeferred");expect(html).not.toContain("<button");
});
