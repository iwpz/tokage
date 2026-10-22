import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "../stores/settings";
import { useProviders } from "../stores/providers";
import { currentAvailableModels } from "./models";
import { paramsOf } from "./modelSpec";
import { runtimeSnapshot,runtimeSubmit,runtimeRequest,type RuntimeConfig,type RuntimeModel } from "./runtime";
import { useRuntime } from "../stores/runtime";
import type { TurnPlan } from "./turn";
/** Resolve desktop inputs once; runtime freezes policy, allocation and authorization. */
export async function createRuntimePlan(sessionId:string,workspace:string,requirement:string,plan:TurnPlan,locked:boolean):Promise<void>{
  if(await runtimeSnapshot(sessionId))throw new Error("This session already has a runtime run. Use its plan and execution controls.");
  const settings=useSettings.getState();
  if(settings.isolation!=="worktree")throw new Error("Shared execution requires worktree isolation.");
  if(settings.gateEnvAllow.length)throw new Error("Custom gate environment permissions are not supported by this runtime version.");
  const models:Record<string,RuntimeModel>={};
  for(const available of currentAvailableModels(sessionId)??[]){
    const provider=useProviders.getState().providers.find(p=>p.id===available.provider);
    if(!provider?.adapter?.caps.includes("STRUCTURED_OUTPUT"))continue;
    models[`${available.provider}:${available.cliModelArg}`]={provider:available.provider,requestedModel:available.cliModelArg,providerPath:settings.providerPaths[provider.id]??null,capabilities:provider.adapter.caps,userLocked:false};
  }
  const provider=useProviders.getState().providers.find(p=>p.id===plan.ref.provider);
  const selected:RuntimeModel={provider:plan.ref.provider,requestedModel:plan.variant.id,providerPath:settings.providerPaths[plan.ref.provider]??null,capabilities:provider?.adapter?.caps??[],userLocked:locked,modelParams:paramsOf(plan.variant)};
  models.executor=selected;models.planner={...selected,userLocked:true};models.reviewer={...selected,userLocked:true};
  const config=await invoke<RuntimeConfig>("runtime_config",{options:{workspace,models,trustWorkspace:true,permissionMode:"workspace-write",concurrency:settings.maxParallel,providerConcurrency:{},budgetUsd:settings.planBudgetUsd>0?settings.planBudgetUsd:null,blockUnknownCost:settings.pauseOnOverrun,timeoutSeconds:settings.agentHardTimeoutMin*60,repairPolicy:"bounded-ladder",contextPolicy:settings.contextMode,coAuthor:settings.tokageCoAuthor}});
  await runtimeSubmit({schemaVersion:1,commandId:crypto.randomUUID(),runId:sessionId,expectedRevision:0,command:{type:"CreateRun",payload:{requirement,config}}});
  const s=await runtimeSnapshot(sessionId);if(!s)throw new Error("Run creation was not persisted");
  await runtimeSubmit(runtimeRequest(s,{type:"GeneratePlan"}));await useRuntime.getState().refresh(sessionId);
}
