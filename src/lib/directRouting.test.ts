import {expect,it,vi} from "vitest";
import type {ModelGroup,ModelRef} from "./models";
import type {Conversation} from "../stores/chat";
import {selectDirectResource} from "./directRouting";
vi.mock("./models",()=>({
  findModel:(groups:ModelGroup[],ref:ModelRef)=>groups.find(g=>g.provider.id===ref.provider)?.models.find(m=>m.key===ref.model),
  recommend:()=>({provider:"OPENCODE",model:"new"}),
}));
const old:ModelRef={provider:"AIDER",model:"old"};
const fresh:ModelRef={provider:"OPENCODE",model:"new"};
const groups=[{provider:{id:"AIDER"},models:[{key:"old"}]},{provider:{id:"OPENCODE"},models:[{key:"new"}]}] as ModelGroup[];
const settings={modelProviderPreferences:{},modelChannelAccess:{}};
const conversation:Conversation={directModel:null,thread:{ref:old,id:"native"},messages:[{id:"previous",role:"assistant",text:"work",ref:old,mode:"direct",status:"done"}]};
it("recommends locally for a new session",()=>{
  expect(selectDirectResource(groups,undefined,null,settings)).toEqual({ref:fresh,reason:"recommended"});
});
it("keeps the previous resource even if a new recommendation or another session's pick changes",()=>{
  expect(selectDirectResource(groups,conversation,fresh,settings)).toEqual({ref:old,reason:"continue"});
});
it("refuses silent failover when the current resource becomes unavailable",()=>{
  expect(selectDirectResource(groups.slice(1),conversation,null,settings)).toEqual({ref:null,reason:"unavailable"});
});
it("an explicit selection overrides continuity, including the same model as the recommendation",()=>{
  expect(selectDirectResource(groups,{...conversation,directModel:fresh},null,settings)).toEqual({ref:fresh,reason:"manual"});
  expect(selectDirectResource(groups.slice(1),{...conversation,directModel:old},null,settings)).toEqual({ref:null,reason:"unavailable"});
});
it("failed turns retain their resource and do not authorize another provider",()=>{
  expect(selectDirectResource(groups,{...conversation,thread:null,messages:conversation.messages.map(m=>({...m,status:"error"}))},null,settings)).toEqual({ref:old,reason:"continue"});
});
