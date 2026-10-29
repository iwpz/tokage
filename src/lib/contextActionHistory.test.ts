import { expect,it } from "vitest";
import { contextHistorySourceIds } from "./contextActionHistory";
import type { ChatMessage,Conversation } from "../stores/chat";
const message:ChatMessage={id:"source",role:"assistant",text:"",mode:"direct",nativeThreadId:"window",nativeThreadSessionId:"session",ref:{provider:"CODEX_CLI",model:"model"}};
const conv:Conversation={thread:{id:"window",ref:message.ref!,mode:"direct"},messages:[message]};
it("history excludes foreign windows, models, roles and sessions, including display aliases",()=>{
 const messages=[message,{...message,id:"alias",reportedModel:"observed alias"},
 {...message,id:"foreign-window",nativeThreadId:"other"},{...message,id:"foreign-session",nativeThreadSessionId:"other"},
 {...message,id:"foreign-model",ref:{...message.ref!,model:"other"}}, {...message,id:"foreign-role",mode:"plan" as const},
 {...message,id:"user",role:"user" as const},message];
 expect(contextHistorySourceIds({...conv,messages},"session")).toEqual(["source","alias"]);
 expect(contextHistorySourceIds({...conv,thread:null},"session")).toEqual([]);
 expect(contextHistorySourceIds(conv,"other")).toEqual([]);
});
it("history stays bounded to the newest 512 unique source ids",()=>{
 const messages=Array.from({length:600},(_,i)=>({...message,id:`source-${i}`}));
 const ids=contextHistorySourceIds({...conv,messages},"session");
 expect(ids).toHaveLength(512);expect(ids[0]).toBe("source-88");expect(ids.at(-1)).toBe("source-599");
});
