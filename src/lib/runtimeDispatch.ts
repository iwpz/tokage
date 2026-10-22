import { useProjects } from "../stores/projects";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "../stores/settings";
import { useProviders } from "../stores/providers";
import { currentAvailableModels } from "./models";
import { paramsOf } from "./modelSpec";
import { runtimeSnapshot,runtimeSubmit,runtimeRequest,type RuntimeConfig,type RuntimeModel } from "./runtime";
import { useRuntime } from "../stores/runtime";
import type { TurnPlan } from "./turn";
import type { Plan } from "./ipc";
/** Resolve desktop inputs once; runtime freezes policy, allocation and authorization. */
export async function createRuntimePlan(sessionId:string,workspace:string,requirement:string,plan:TurnPlan,locked:boolean,imported?:Plan|string):Promise<void>{
  if(await runtimeSnapshot(sessionId))throw new Error("This session already has a runtime run. Use its plan and execution controls.");
  const project=useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sessionId));
  if(project?.trusted!==true)throw new Error("Workspace trust is required before creating a runtime run.");
  const settings=useSettings.getState();
  if(["luna","deepseek"].includes(settings.debugProvider.output))throw new Error("Shared runtime requires an explicit real provider/model; paid surrogate debug routing is unsupported.");
  if(settings.isolation!=="worktree")throw new Error("Shared execution requires worktree isolation.");
  const models:Record<string,RuntimeModel>={};
  for(const available of currentAvailableModels(sessionId)??[]){
    const provider=useProviders.getState().providers.find(p=>p.id===available.provider);
    if(!provider?.adapter?.caps.includes("STRUCTURED_OUTPUT"))continue;
    models[`${available.provider}:${available.cliModelArg}`]={provider:available.provider,requestedModel:available.cliModelArg,providerPath:settings.providerPaths[provider.id]??null,capabilities:provider.adapter.caps,userLocked:false};
  }
  const provider=useProviders.getState().providers.find(p=>p.id===plan.ref.provider);
  const selected:RuntimeModel={provider:plan.ref.provider,requestedModel:plan.variant.id,providerPath:settings.providerPaths[plan.ref.provider]??null,capabilities:provider?.adapter?.caps??[],userLocked:locked,modelParams:paramsOf(plan.variant)};
  models.executor=selected;models.planner={...selected,userLocked:true};models.reviewer={...selected,userLocked:true};
  const config=await invoke<RuntimeConfig>("runtime_config",{options:{workspace,models,trustWorkspace:true,permissionMode:"workspace-write",concurrency:settings.maxParallel,providerConcurrency:{},budgetUsd:settings.planBudgetUsd>0?settings.planBudgetUsd:null,blockUnknownCost:settings.pauseOnOverrun,timeoutSeconds:settings.agentHardTimeoutMin*60,repairPolicy:"bounded-ladder",contextPolicy:settings.contextMode,coAuthor:settings.tokageCoAuthor,gateEnvAllow:settings.gateEnvAllow}});
  await runtimeSubmit({schemaVersion:1,commandId:crypto.randomUUID(),runId:sessionId,expectedRevision:0,command:{type:"CreateRun",payload:{requirement,config}}});
  const s=await runtimeSnapshot(sessionId);if(!s)throw new Error("Run creation was not persisted");
  const command=imported?{type:"ImportPlan",payload:{text:typeof imported==="string"?imported:await invoke<string>("runtime_render_plan",{plan:imported})}}:{type:"GeneratePlan"};
  try{await runtimeSubmit(runtimeRequest(s,command));}finally{await useRuntime.getState().refresh(sessionId);}
}
