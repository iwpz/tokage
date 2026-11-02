import {directRecovery,type DirectSnapshot} from "./direct";
import {projectDirectRecovery} from "./directProjection";
import {isStreaming,useChat} from "../stores/chat";
import {sessionCwd,useProjects,type SessionWorktree} from "../stores/projects";
import {gitInfo,inTauri} from "./ipc";
import {demoIdentity} from "./demoIdentity";
import i18n from "../i18n";

const restoring=new Map<string,Promise<DirectSnapshot>>();

/** Restoration is observation plus UI persistence. No send, approval decision,
 * trust change, queue drain, worktree creation or command replay takes place. */
export async function restoreDirectSession(projectId:string,sessionId:string,open=false,freezeStartup=false):Promise<DirectSnapshot>{
  const pending=restoring.get(sessionId);
  if(pending){await pending;return restoreDirectSession(projectId,sessionId,open,freezeStartup);}
  const work=restore(projectId,sessionId,open,freezeStartup);
  restoring.set(sessionId,work);
  try{return await work;}finally{restoring.delete(sessionId);}
}

async function restore(projectId:string,sessionId:string,open:boolean,freezeStartup:boolean):Promise<DirectSnapshot>{
  const active={projectId:useProjects.getState().activeProjectId,sessionId:useProjects.getState().activeSessionId};
  const original=useChat.getState().conversations[sessionId];
  if(isStreaming(original))throw new Error(i18n.t("workspace.direct.recoveryActive"));
  const project=useProjects.getState().projects.find(p=>p.id===projectId);
  if(!project)throw new Error(i18n.t("workspace.direct.recoveryInvalid"));
  const existing=project.sessions.find(s=>s.id===sessionId);
  const snapshot=await directRecovery(sessionId,project.id,project.path,existing?sessionCwd(project,sessionId):undefined);
  if(snapshot.active)throw new Error(i18n.t("workspace.direct.recoveryActive"));
  if(!snapshot.turns.length)return snapshot;
  const main=snapshot.turns.filter(t=>!t.record.request.conversation?.isolated).at(-1)??snapshot.turns.at(-1)!;
  const {record}=main;
  const cwd=record.request.cwd??record.cwd;
  let worktree=existing?.worktree;
  if(!existing&&cwd!==project.path&&record.cwd!==(snapshot.canonicalProjectPath??project.path)){
    const saved=record.request.conversation?.worktree;
    const underRoot=saved&&(cwd===saved.root||cwd.startsWith(saved.root.replace(/[\\/]+$/,"")+"/")||cwd.startsWith(saved.root.replace(/[\\/]+$/,"")+"\\"));
    if(saved?.cwd===cwd&&saved.root&&typeof saved.branch==="string"&&underRoot)worktree={...saved};
    else{
      const info=await gitInfo(cwd);
      if(!info.linkedWorktree||!info.root||!info.branch)throw new Error(i18n.t("workspace.direct.recoveryWorkspace"));
      worktree={root:info.root,cwd,branch:info.branch} satisfies SessionWorktree;
    }
  }
  if(existing&&sessionCwd(project,sessionId)!==cwd&&(snapshot.canonicalExpectedCwd??sessionCwd(project,sessionId))!==record.cwd)throw new Error(i18n.t("workspace.direct.recoveryWorkspace"));
  const rebuilt=projectDirectRecovery(original,snapshot,freezeStartup);
  if(useChat.getState().conversations[sessionId]!==original||useProjects.getState().projects.find(p=>p.id===projectId)!==project)throw new Error(i18n.t("workspace.direct.handoffStale"));
  if(open&&(useProjects.getState().activeProjectId!==active.projectId||useProjects.getState().activeSessionId!==active.sessionId))throw new Error(i18n.t("workspace.direct.handoffStale"));
  const first=snapshot.turns[0].record;
  useProjects.getState().attachDirectSession(projectId,{
    id:sessionId,title:first.request.conversation?.sessionTitle??first.request.prompt.split("\n")[0].slice(0,60),
    state:rebuilt.recoveryUnresolved?"blocked":"draft",updatedAt:(record.finishedAt??record.createdAt)*1000,worktree,
  },open);
  useChat.setState(s=>({conversations:{...s.conversations,[sessionId]:rebuilt}}));
  return snapshot;
}

export async function restoreExistingDirectSessions():Promise<void>{
  if(!inTauri)return;
  for(const project of useProjects.getState().projects){
    for(const session of project.sessions){
      if(session.archived||demoIdentity(session.id))continue;
      try{await restoreDirectSession(project.id,session.id,false,true);}
      catch(error){
        useChat.setState(s=>({conversations:{...s.conversations,[session.id]:{...(s.conversations[session.id]??{messages:[],thread:null}),recoveryError:String(error)}}}));
      }
    }
  }
}
