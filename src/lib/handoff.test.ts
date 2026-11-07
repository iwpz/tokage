import {expect,it} from "vitest";
import {buildHandoffBrief,handoffSource} from "./handoff";
import type {Conversation} from "../stores/chat";

it("keeps recorded evidence separate from authorization and discloses truncation",()=>{
  const conv:Conversation={thread:null,messages:[
    ...Array.from({length:15},(_,i)=>({id:String(i),role:"user" as const,text:`Goal ${i}`})),
    {id:"answer",role:"assistant",mode:"direct",text:"x".repeat(2000),status:"error"},
    {id:"notice",role:"assistant",text:"Internal routing notice",origin:"tokage"},
  ]};
  const brief=buildHandoffBrief(conv,"/workspace");
  expect(brief).toContain("[4 earlier messages omitted");
  expect(brief).toContain("Message truncated");
  expect(brief).toContain("assistant (error)");
  expect(brief).toContain("not a new authorization");
  expect(brief).not.toContain("Internal routing notice");
  expect(handoffSource(conv)).toBe("answer");
});
