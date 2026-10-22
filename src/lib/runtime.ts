/** Versioned runtime transport. No retries, scheduling or local acceptance decisions. */
import { invoke } from "@tauri-apps/api/core";
import type { ChatEventKind, ModelParams, Plan } from "./ipc";
export interface RuntimeModel {provider:string;requestedModel:string;providerPath:string|null;capabilities:string[];userLocked:boolean;modelParams?:ModelParams|null}
export interface RuntimeConfig {
  source:string;policyVersion:string;workspace:string;targetIdentity:string;targetReference?:string|null;
  scope:string[];models:Record<string,RuntimeModel>;commandPolicyHash:string;trustWorkspace:boolean;
  permissionMode:string;concurrency:number;providerConcurrency?:Record<string,number>;budgetUsd:number|null;
  blockUnknownCost:boolean;timeoutSeconds:number;repairPolicy:string;contextPolicy:string;
  gateEnvAllow?:string[];confirmedCommands:string[];customPackHashes:string[];coAuthor:boolean;planningPolicy?:unknown;
}
export interface RuntimeApproval {id:string;turnId:string;command:string;recoverable:boolean;revision:number;confirmationDigest:string;decision:string|null}
export interface RuntimeTurn {id:string;role:string;taskId:string|null;status:string;requested:RuntimeModel;observedModel:string|null;observedProvider:string|null;costUsd:number|null;lastSequence:number;approval:RuntimeApproval|null;error:string|null;ended:boolean;cleanupConfirmed:boolean}
export interface RuntimeSnapshot {
  schemaVersion:1;runId:string;revision:number;state:string;requirement:string;config:RuntimeConfig;configHash:string;
  plan?:{plan:Plan;assignments:Record<string,RuntimeModel>;confirmationDigest:string;issues:unknown[]};approvedDigest?:string;
  turns?:Record<string,RuntimeTurn>;execution?:{sequence:number;tasks:Record<string,{state:string;assignment:RuntimeModel;attempts:string[];error:string|null;deviation:boolean;repairStep:string}>};
  work?:{integration?:{root:string;cwd:string;branch:string};integrationHead?:string;attempts:Record<string,{error?:string|null;commit?:string|null;integratedHead?:string|null;gates:unknown[];manualReceipt?:{commandId:string}}>};
  delivery?:{candidate?:{head:string};review?:unknown;confirmationDigest?:string;artifact?:string;intent?:{error?:string|null;completed:boolean}};
}
export interface RuntimeEvent {schemaVersion:1;runId:string;sequence:number;revision:number;turnId:string|null;attemptId:string|null;kind:string}
export interface TranscriptEvent {turnId:string;sequence:number;event:ChatEventKind}
export interface RuntimeCommand {type:string;payload?:Record<string,unknown>}
export interface RuntimeRequest {schemaVersion:1;commandId:string;runId:string;expectedRevision:number;command:RuntimeCommand}
export const runtimeSnapshot=(runId:string)=>invoke<RuntimeSnapshot|null>("runtime_snapshot",{runId});
export const runtimeList=(workspace?:string)=>invoke<RuntimeSnapshot[]>("runtime_list",{workspace});
export const runtimeEvents=(runId:string,after:number,limit=200)=>invoke<RuntimeEvent[]>("runtime_events",{runId,after,limit});
export const runtimeTranscript=(runId:string,turnId:string,after:number,limit=200)=>invoke<TranscriptEvent[]>("runtime_transcript",{runId,turnId,after,limit});
// Command payload field names are the original Rust schema-v1 snake_case; outer DTOs use camelCase.
export const runtimeSubmit=(request:RuntimeRequest)=>{
 const keys:Record<string,string>={confirmationDigest:"confirmation_digest",confirmedCommands:"confirmed_commands",taskId:"task_id",turnId:"turn_id",approvalId:"approval_id",outputPath:"output_path",sourceDigest:"source_digest"};
 const command=request.command.payload?{...request.command,payload:Object.fromEntries(Object.entries(request.command.payload).map(([key,value])=>[keys[key]??key,value]))}:request.command;
 return invoke<{run_id:string;command_id:string;revision:number}>("runtime_submit",{request:{...request,command}});
};
export const runtimeRequest=(snapshot:RuntimeSnapshot,command:RuntimeCommand):RuntimeRequest=>({schemaVersion:1,commandId:crypto.randomUUID(),runId:snapshot.runId,expectedRevision:snapshot.revision,command});
export const runtimeWorkerError=(runId:string)=>invoke<string|null>("runtime_worker_error",{runId});
export interface TaskInspection {workspace:{root:string;cwd:string;branch:string};sourceDigest:string;confirmationDigest:string}
export const runtimeInspectTask=(runId:string,taskId:string)=>invoke<TaskInspection>("runtime_inspect_task",{runId,taskId});
