import { useProjects, type MainSession } from "./projects";
import { create } from "zustand";
import { runtimeSnapshot, runtimeWorkerError, runtimeEvents, runtimeTranscript, runtimeRequest, runtimeSubmit, type RuntimeCommand, type RuntimeSnapshot, type TranscriptEvent } from "../lib/runtime";
interface Projection {turnOrder?:string[];snapshot:RuntimeSnapshot;cursor:number;transcripts:Record<string,TranscriptEvent[]>;turnCursors:Record<string,number>}
interface RuntimeState {runs:Record<string,Projection>;errors:Record<string,string>;refresh:(runId:string)=>Promise<void>;command:(runId:string,command:RuntimeCommand)=>Promise<void>}
const inflight=new Map<string,Promise<void>>();
/** All state is a projection. This Store cannot set task states, grant acceptance or dispatch. */
export const useRuntime=create<RuntimeState>((set,get)=>({
  runs:{},errors:{},
  refresh:(runId)=>{
    const existing=inflight.get(runId);if(existing)return existing;
    const promise=(async()=>{
      const s=await runtimeSnapshot(runId);if(!s)return;
      if(s.schemaVersion!==1)throw new Error("Unsupported runtime schema");
      const workerError=await runtimeWorkerError(runId);
      const previous=get().runs[runId];
      // Snapshot may be ahead of the event page. Replay cursor is advanced only by actual events.
      let cursor=previous?.cursor??0;
      const turnOrder=[...(previous?.turnOrder??[])];
      const events=await runtimeEvents(runId,cursor);
      for(const e of events){if(e.sequence<=cursor)continue;if(e.schemaVersion!==1||e.runId!==runId||e.sequence!==cursor+1)throw new Error("Runtime event gap");cursor=e.sequence;if(e.turnId&&!turnOrder.includes(e.turnId))turnOrder.push(e.turnId);}
      const transcripts={...previous?.transcripts};const turnCursors={...previous?.turnCursors};
      for(const turn of Object.values(s.turns??{})){
        let after=turnCursors[turn.id]??0;
        if(after>=turn.lastSequence)continue;
        const page=await runtimeTranscript(runId,turn.id,after);
        const rows=[...(transcripts[turn.id]??[])];
        for(const e of page){if(e.sequence<=after)continue;if(e.turnId!==turn.id||e.sequence!==after+1)throw new Error("Runtime transcript gap");rows.push(e);after=e.sequence;}
        transcripts[turn.id]=rows;turnCursors[turn.id]=after;
      }
      set(state=>({runs:{...state.runs,[runId]:{snapshot:!previous||s.revision>=previous.snapshot.revision?s:previous.snapshot,cursor,transcripts,turnCursors,turnOrder}},errors:{...state.errors,[runId]:workerError??""}}));
      const authoritative=get().runs[runId].snapshot;
      const display:MainSession["state"]=authoritative.state==="Delivered"?"done":["Blocked","Stopped"].includes(authoritative.state)?"blocked":["AwaitingApproval","Approved","AwaitingReview","AwaitingMerge"].includes(authoritative.state)?"review":["Draft","Planning"].includes(authoritative.state)?"draft":"running";
      const session=useProjects.getState().projects.flatMap(p=>p.sessions).find(item=>item.id===runId);
      if(session&&session.state!==display)useProjects.getState().setSessionState(runId,display);

    })().catch(error=>{set(s=>({errors:{...s.errors,[runId]:String(error)}}));throw error;}).finally(()=>inflight.delete(runId));
    inflight.set(runId,promise);return promise;
  },
  command:async(runId,command)=>{
    const s=get().runs[runId]?.snapshot??await runtimeSnapshot(runId);if(!s)throw new Error("Run not found");
    await runtimeSubmit(runtimeRequest(s,command));await get().refresh(runId);
  },
}));
/** A view only observes. Disposing it never submits CancelRun or ResumeRun. */
export function observeRuntime(runId:string,onError?:(error:unknown)=>void):()=>void {
  let disposed=false;let timer:ReturnType<typeof setTimeout>|undefined;
  const poll=async()=>{try{await useRuntime.getState().refresh(runId);}catch(e){onError?.(e);}finally{if(!disposed)timer=setTimeout(()=>void poll(),350);}};
  void poll();return()=>{disposed=true;if(timer)clearTimeout(timer);};
}
