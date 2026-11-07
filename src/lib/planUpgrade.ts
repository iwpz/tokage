import {useChat,isStreaming} from "../stores/chat";
import {sessionCwd,useProjects} from "../stores/projects";
import {directHistory} from "./direct";
import {gitInfo} from "./ipc";
import {handoffSource} from "./handoff";
import {planTurn} from "./turn";
import {createRuntimePlan} from "./runtimeDispatch";
import i18n from "../i18n";

/** Create a linked planning run. The original session, changes and permissions
 * remain intact; the runtime still requires ordinary plan review and approval. */
export async function upgradeDirectSession(sourceId:string,requirement:string,brief:string,sourceMessageId:string):Promise<string>{
  const project=useProjects.getState().projects.find(p=>p.sessions.some(s=>s.id===sourceId));
  const original=useChat.getState().conversations[sourceId];
  const check=()=>{
    const current=useChat.getState().conversations[sourceId];
    if(!current||current!==original||isStreaming(current)||current.messages.some(m=>m.status==="waiting_approval")||handoffSource(current)!==sourceMessageId)throw new Error(i18n.t("workspace.direct.handoffStale"));
  };
  check();
  if(!project||project.trusted!==true||!requirement.trim()||!brief.trim())throw new Error(i18n.t("workspace.direct.upgradeUnavailable"));
  const cwd=sessionCwd(project,sourceId);
  const history=await directHistory(sourceId);
  if(history.some(r=>r.finishedAt==null))throw new Error(i18n.t("workspace.direct.unresolved"));
  const git=await gitInfo(cwd);
  if(!git.isRepo||!git.hasCommits||git.detached||git.changes>0)throw new Error(i18n.t("workspace.direct.upgradeClean"));
  check();
  const plan=planTurn(requirement,"plan",null,"plan",sourceId);
  if(!plan)throw new Error(i18n.t("workspace.composer.noModels"));
  const source=project.sessions.find(s=>s.id===sourceId)!;
  const id=useProjects.getState().createSession(project.id,requirement.split("\n")[0].slice(0,60),source.worktree);
  useChat.setState(s=>({conversations:{...s.conversations,[id]:{messages:[],thread:null,linkedFrom:{sessionId:sourceId,kind:"plan-upgrade"}}}}));
  await createRuntimePlan(id,cwd,`${requirement}\n\n${brief}`,plan,false);
  return id;
}
