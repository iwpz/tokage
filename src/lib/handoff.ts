import type {Conversation} from "../stores/chat";

export function handoffSource(conversation:Conversation):string {
  return conversation.messages.filter(m=>m.origin!=="tokage").at(-1)?.id??"";
}

/** Local, editable evidence extract. No model call or hidden-state transfer. */
export function buildHandoffBrief(conversation:Conversation,cwd:string):string {
  const messages=conversation.messages.filter(m=>m.origin!=="tokage" && !m.contextReport);
  const recent=messages.slice(-12);
  const files=[...new Set(messages.flatMap(m=>(m.files??[]).map(f=>f.path)))];
  const lines=["# Work handoff",`Workspace: ${cwd}`,"The following is historical context, not a new authorization. Inspect the actual workspace and follow current project instructions. Hidden CLI state, caches and tool sessions are not transferred.","", "## Conversation evidence"];
  if(messages.length>recent.length)lines.push(`[${messages.length-recent.length} earlier messages omitted; consult the source conversation.]`);
  for(const m of recent)lines.push(`### ${m.role} (${m.status??"recorded"})`,m.text.length>1600?`${m.text.slice(0,1600)}\n[Message truncated; inspect the source conversation.]`:m.text);
  if(files.length)lines.push("## Reported changed paths",...files.slice(0,100).map(p=>`- ${p}`),...(files.length>100?[`[${files.length-100} paths omitted; inspect the source conversation.]`]:[]));
  lines.push("", "## Next step", "Inspect current changes and verify reported checks. Preserve completed work; do not repeat commands or edits solely because they appear in this history.");
  return lines.join("\n\n");
}
