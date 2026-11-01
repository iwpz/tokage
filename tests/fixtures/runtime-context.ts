import { runtimeUiSnapshot } from "./runtime-ui";
import type { ResolvedContextInputs } from "../../src/lib/contextInputs";
import type { RuntimeContextPreparation } from "../../src/lib/runtime";
export const taskContextInputs = {
 params:{remainingTurns:1,apiCallsPerTurn:1,growthPerTurn:4000,minSaving:.15,cacheReadMultiplier:.02,cacheWriteMultiplier:1,pIn:.15,pOut:.6},
 preview:{recommendedAction:"FRESH",explanation:"Retain current task",continueCostUsd:.02,compactCostUsd:.01,freshCostUsd:.009,cooldownRemainingTurns:0,cooldownBlocked:false},
 cooldown:{known:true},compactIntent:{action:"FRESH"},
} as ResolvedContextInputs;
export const taskContextPreparation:RuntimeContextPreparation={sourceTurn:"a",targetTurn:"next:a",sourceRevision:7,configHash:"hash",planDigest:"digest",sourceDigest:"source",policy:"suggest",status:"PREPARED",preview:taskContextInputs,requireNewWindow:true,applies:true,boundAt:null,nativeConfirmed:false,userAccepted:true};
export function taskContextSnapshot(){
 const s=structuredClone(runtimeUiSnapshot);s.state="Blocked";s.config.contextPolicy="suggest";s.approvedDigest="digest";
 const model=s.config.models.executor;
 s.turns={a:{...s.turns!.planner,id:"a",role:"executor",taskId:"T1",windowOrigin:"a",threadId:"window-a",requested:model}};
 s.execution={sequence:1,tasks:{T1:{state:"BLOCKED",assignment:model,attempts:["a"],error:null,deviation:false,repairStep:"INITIAL",contextActions:[]}}};
 return s;
}
