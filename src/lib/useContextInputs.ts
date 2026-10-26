import { useEffect, useMemo, useState } from "react";
import { inTauri, resolveContextInputs } from "./ipc";
import type { ContextInputRequest, ResolvedContextInputs } from "./contextInputs";
import { useKnowledge } from "../stores/knowledge";

/** Read-only resolution; late responses can never replace another window's inputs. */
export function useContextInputs(request: ContextInputRequest | null, enabled: boolean) {
  const packs = useKnowledge(s=>s.customPacks);
  const overrides = useKnowledge(s=>s.modelOverrides);
  const applyingUpdate = useKnowledge(s=>s.applyingUpdate);
  const knowledgeKey = useMemo(()=>JSON.stringify({applyingUpdate,packs: packs.map(p=>[p.id,p.enabled,p.pack]),overrides}),[packs,overrides,applyingUpdate]);
  const payload = JSON.stringify(request);
  const key = payload + knowledgeKey;
  const [state,setState] = useState<{key:string;inputs:ResolvedContextInputs|null}>({key:"",inputs:null});
  useEffect(()=>{
    if (!enabled || applyingUpdate || !inTauri || payload==="null") return;
    let current = true;
    void resolveContextInputs(JSON.parse(payload)).then(inputs=>{
      if(current) setState({key,inputs});
    }).catch(error=>{
      console.warn("[context] input resolution unavailable",error);
      if(current) setState({key,inputs:null});
    });
    return ()=>{current=false;};
  },[key,payload,enabled,applyingUpdate]);
  return enabled && state.key===key ? state.inputs : null;
}
