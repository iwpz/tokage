import {reduceChatMessage,type ChatMessage,type Conversation,type ApprovalPart} from "../stores/chat";
import type {DirectSnapshot,DirectRecord} from "./direct";
import type {ChatEventKind,ProviderId} from "./ipc";
import type {NativeThread} from "./nativeSessionResume";
import i18n from "../i18n";

function sourceRef(record:DirectRecord){
  return record.request.conversation?.ref??{provider:record.request.provider as ProviderId,model:record.request.model};
}

/** Pure projection: replay data into display state, never into execution, approval,
 * context-saving, cost settlement or queue-drain callbacks. */
export function projectDirectRecovery(original:Conversation|undefined,snapshot:DirectSnapshot,freezeStartup=false):Conversation{
  if(snapshot.active)throw new Error(i18n.t("workspace.direct.recoveryActive"));
  const base=original??{messages:[],thread:null};
  if(!snapshot.turns.length)return base;
  const messages=[...base.messages];
  if(!messages.length&&snapshot.turns[0].record.request.history?.length){
    snapshot.turns[0].record.request.history!.forEach((m,index)=>{
      if(m.role!=="user"&&m.role!=="assistant")throw new Error(i18n.t("workspace.direct.recoveryInvalid"));
      messages.push({id:`direct:${snapshot.turns[0].record.turnId}:context:${index}`,role:m.role,text:m.text,recovered:true,
        ...(m.role==="assistant"?{parts:[{type:"warning" as const,message:i18n.t("workspace.direct.recoveredContext")}]}:{})});
    });
  }
  let thread:NativeThread|null=base.thread;
  const represented=new Set<string>();
  const used=new Set<string>();
  for(const turn of snapshot.turns){
    const {record,events,partialTail}=turn;
    if(record.sessionId!==snapshot.sessionId||record.schemaVersion!==1)throw new Error(i18n.t("workspace.direct.recoveryInvalid"));
    const metadata=record.request.conversation;
    if(metadata&&(!metadata.userMessageId||!metadata.assistantMessageId||metadata.userMessageId===metadata.assistantMessageId||used.has(metadata.assistantMessageId)))throw new Error(i18n.t("workspace.direct.recoveryInvalid"));
    let ref=sourceRef(record);
    const variant={id:record.request.model,effort:record.request.effort??null,fast:record.request.fast??false,tier:record.request.tier??null};
    let prior=messages.find(m=>m.role==="assistant"&&(m.turnId===record.turnId||(metadata&&m.id===metadata.assistantMessageId)));
    // Legacy records predate stable message IDs. Match only an unambiguous
    // prompt/model pair, using saved response text to distinguish repeated asks.
    if(!prior&&!metadata){
      const reply=events.filter((e):e is Extract<ChatEventKind,{kind:"text"}>=>e.kind==="text").map(e=>e.text).join("");
      const window=events.find((e):e is Extract<ChatEventKind,{kind:"thread"}>=>e.kind==="thread")?.id;
      const candidates=messages.filter((m,index)=>m.role==="assistant"&&!m.turnId&&!used.has(m.id)&&m.mode==="direct"&&m.model===record.request.model&&(m.provider??m.ref?.provider)===record.request.provider&&messages[index-1]?.role==="user"&&(messages[index-1]?.text===record.request.prompt||(!!window&&m.nativeThreadId===window&&!!m.text&&reply.startsWith(m.text)))&&(!m.text||reply.startsWith(m.text)));
      if(candidates.length>1)throw new Error(i18n.t("workspace.direct.recoveryAmbiguous"));
      prior=candidates[0];
    }
    if(!metadata&&prior?.ref)ref=prior.ref;
    const assistantId=prior?.id??metadata?.assistantMessageId??`direct:${record.turnId}:assistant`;
    const priorIndex=prior?messages.indexOf(prior):-1;
    const userId=metadata?.userMessageId??(priorIndex>0&&messages[priorIndex-1].role==="user"?messages[priorIndex-1].id:`direct:${record.turnId}:user`);
    if(messages.some(m=>(m.id===assistantId&&m.role!=="assistant")||(m.id===userId&&m.role!=="user")))throw new Error(i18n.t("workspace.direct.recoveryInvalid"));
    const localTiming=prior?.startedAt!=null&&prior.endedAt!=null&&prior.endedAt>=prior.startedAt;
    let message:ChatMessage={...prior,
      id:assistantId,turnId:record.turnId,recovered:true,role:"assistant",text:"",mode:"direct",auto:false,
      provider:ref.provider,ref,model:record.request.model,modelName:prior?.modelName??record.request.model,variant,
      cwd:record.request.cwd??record.cwd,contextReport:metadata?.contextReport??prior?.contextReport,parts:[],files:[],error:undefined,
      startedAt:localTiming?prior!.startedAt:record.createdAt*1000,
      endedAt:record.finishedAt==null?undefined:localTiming?prior!.endedAt:record.finishedAt*1000,
    };
    let completion:Extract<ChatEventKind,{kind:"done"}>|undefined;
    for(const event of events){
      if(event.kind==="done")completion=event;
      else if(event.kind==="model")message={...message,reportedModel:event.name}; // no model-learning writes on replay
      else if(event.kind!=="error")message=reduceChatMessage(message,{...event,turnId:record.turnId},snapshot.sessionId);
    }
    const unfinished=message.parts?.some(p=>p.type==="tool"&&!p.done);
    message.parts=message.parts?.map(part=>{
      if(part.type==="tool")return {...part,done:true}; // ok:null stays unknown
      if(part.type!=="approval"||part.resolved)return part;
      const matching=(message.parts??[]).filter((p):p is ApprovalPart=>p.type==="approval"&&p.command===part.command);
      const audit=snapshot.approvals.filter(a=>a.turnId===record.turnId&&a.command===part.command&&["allow_once","allow_session","allow_project","deny"].includes(a.decision));
      // The audit table has no tool ID: do not guess when a turn repeats a command.
      const latest=matching.length===1?audit.sort((a,b)=>(b.id??0)-(a.id??0))[0]:undefined;
      return latest?{...part,resolved:true,decision:latest.decision}:part;
    });
    if(completion)message.usage={input:completion.inputTokens,output:completion.outputTokens,cacheRead:completion.cacheReadTokens,cacheWrite:completion.cacheWriteTokens,apiCalls:completion.apiCalls,contextTokens:completion.contextTokens,costUsd:completion.costUsd,premiumRequests:completion.premiumRequests,credits:completion.credits,source:completion.source};
    const pending=message.parts?.some(p=>p.type==="approval"&&!p.resolved);
    if(record.finishedAt==null||partialTail){
      message.status="error";message.error=record.error??i18n.t("workspace.direct.interrupted");
    }else if(record.status==="completed"&&completion){
      message.status=pending?"waiting_approval":"done";
    }else if(record.status==="awaiting_approval"&&pending){
      message.status="waiting_approval";
    }else if(record.status==="stopped")message.status="cancelled";
    else{
      message.status="error";message.error=record.error??i18n.t(record.status==="awaiting_approval"?"workspace.direct.approvalRecorded":"workspace.direct.interrupted");
    }
    if(partialTail)message.parts=[...message.parts??[],{type:"warning",message:i18n.t("workspace.direct.partialJournal")}];
    if(unfinished)message.parts=[...message.parts??[],{type:"warning",message:i18n.t("workspace.direct.toolOutcomeUnknown")}];
    // Keep model-generated reports from being mistaken for the user's own text.
    const user:ChatMessage={id:userId,role:"user",text:metadata?.userText??(priorIndex>0?messages[priorIndex-1].text:record.request.prompt),turnId:record.turnId,recoveredRawInput:!metadata&&priorIndex<0};
    const existingUser=messages.findIndex(m=>m.id===userId);
    if(existingUser>=0)messages[existingUser]={...messages[existingUser],...user};
    else if(priorIndex>=0)messages.splice(priorIndex,0,user);
    else messages.push(user);
    const assistantIndex=messages.findIndex(m=>m.role==="assistant"&&m.id===assistantId);
    if(assistantIndex>=0)messages[assistantIndex]=message;else messages.push(message);
    represented.add(record.turnId);used.add(message.id);
    if(!metadata?.isolated){
      const window=message.nativeThreadId??(!record.request.fork?record.request.resume:null);
      // New metadata records make the native window's ownership explicit. For
      // old records only keep a window whose identity was already known locally.
      thread=window&&(metadata||base.thread?.id===window)?{id:window,ref,mode:"direct"}:null;
    }
  }
  // A UI-only in-flight message without a durable intent cannot be relabelled as
  // success by recovery, nor silently become an automatic retry.
  const settled=messages.map(m=>m.role==="assistant"&&m.status==="streaming"&&!represented.has(m.turnId??"")?{...m,status:"error" as const,error:i18n.t("workspace.direct.interrupted")}:m);
  const advanced=base.directTip!==snapshot.tip;
  const lastMain=snapshot.turns.filter(t=>!t.record.request.conversation?.isolated).at(-1);
  const lastRef=lastMain?(settled.find(m=>m.role==="assistant"&&m.turnId===lastMain.record.turnId)?.ref??sourceRef(lastMain.record)):undefined;
  const lastVisible=settled.filter(m=>m.origin!=="tokage").at(-1)?.id;
  const handoffValid=!!base.directHandoff&&base.directHandoff.sourceMessageId===lastVisible;
  return {...base,messages:settled,thread,directTip:snapshot.tip,
    ...((advanced||freezeStartup)&&lastMain&&lastRef?{directModel:handoffValid?base.directModel:base.directModel?lastRef:null,directHandoff:handoffValid?base.directHandoff:undefined,
      recoveredStartup:{ref:lastRef,variant:{id:lastMain.record.request.model,effort:lastMain.record.request.effort??null,fast:lastMain.record.request.fast??false,tier:lastMain.record.request.tier??null}}}:{}),
    recoveryUnresolved:snapshot.turns.some(t=>t.record.finishedAt==null||t.partialTail),recoveryError:undefined,pendingCompact:advanced?undefined:base.pendingCompact,autoCompactDismissedTurnId:advanced?undefined:base.autoCompactDismissedTurnId};
}
