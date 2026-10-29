import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
vi.mock("react-i18next",async importOriginal=>({...await importOriginal<object>(),useTranslation:()=>({t:(key:string)=>key})}));
import { CompactFailureRecovery } from "./CompactFailureRecovery";
import type { ChatMessage } from "../../stores/chat";
const message:ChatMessage={id:"failed",role:"assistant",text:"",status:"error",compactAction:{actionId:1,method:"fresh",state:"failed",failureReceipt:"error"}};
it("shows unsaved failure status and blocks requeue/dismiss until receipt confirmation",()=>{
 const html=renderToStaticMarkup(<CompactFailureRecovery sessionId="s" message={message}/>);
 expect(html).toContain("workspace.context.failureNotSaved");expect(html).toContain("workspace.context.retryReceipt");
 expect(html.match(/disabled=""/g)).toHaveLength(2);
 const saved=renderToStaticMarkup(<CompactFailureRecovery sessionId="s" message={{...message,compactAction:{...message.compactAction!,failureReceipt:"saved"}}}/>);
 expect(saved).toContain("workspace.context.failureSaved");expect(saved).not.toContain("disabled=");
});
it("keeps completed actions and dismissed failures out of the recovery controls",()=>{
 expect(renderToStaticMarkup(<CompactFailureRecovery sessionId="s" message={{...message,compactAction:{...message.compactAction!,state:"completed"}}}/>)).toBe("");
 const html=renderToStaticMarkup(<CompactFailureRecovery sessionId="s" message={{...message,compactAction:{...message.compactAction!,recovery:"ignored"}}}/>);
 expect(html).toContain("workspace.context.recoveryIgnored");expect(html).not.toContain("<button");
});
