//! Exact pre-follow-up prompt shared by scheduling estimates and native dispatch.
use crate::*;
use tokage_orchestration as orchestration;
pub(crate) fn build(s: &Snapshot, task_id: &str) -> Result<String> {
    let p = s.plan.as_ref().ok_or(Error::Transition)?;
    let task = p
        .plan
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or(Error::Transition)?;
    let upstream = task
        .depends_on
        .iter()
        .filter_map(|id| {
            s.execution
                .as_ref()?
                .tasks
                .get(id)?
                .attempts
                .last()
                .and_then(|aid| s.work.as_ref()?.attempts.get(aid)?.report.as_ref())
                .map(|report| orchestration::UpstreamArtifact {
                    task_id: id.clone(),
                    title: id.clone(),
                    report: report.to_markdown(),
                    changed_files: report.changed_files_list.clone(),
                })
        })
        .collect();
    let brief = orchestration::assemble_task_brief(&orchestration::TaskBriefInput {
        task: task.clone(),
        contracts: p.plan.contracts.clone(),
        upstream_artifacts: upstream,
        failure_evidence: s.execution.as_ref().unwrap().tasks[task_id].error.clone(),
    });
    tokage_context::discipline::validate_task_brief_header(&brief)
        .map_err(|e| Error::Invalid(e.to_string()))?;
    // `active` above has checked the current frozen plan approval. The original request
    // can still contain planning-stage instructions, so finish with the actual phase
    // and its exact authorization rather than leaving that historical request last.
    let prompt = format!(
        "{brief}\n\n## Original request (historical context)\n{}\n\n\
         ## Current authorized phase: execution\n\
         Tokage has already received explicit approval for the frozen plan. \
         Approved plan digest: {}\nAssigned task: {}\n\
         Execute only the assigned task in this brief now: implement its deliverables, \
         run its acceptance commands, then return the required Task report. \
         Do not treat this as a new planning request or ask again for plan approval. \
         Earlier instructions to plan first or wait for plan approval describe the completed planning phase. \
         All functional requirements, contracts, frozen tests and write-scope restrictions still apply. \
         Do not bypass CLI permission prompts; new command approvals must use the normal approval channel. \
         Do not commit, change Git metadata, edit ignored or unrelated files. \
         Tokage independently performs acceptance, commits and delivery.",
        serde_json::to_string(&s.requirement).map_err(|e|Error::Invalid(e.to_string()))?,
        s.approved_digest.as_ref().ok_or(Error::Transition)?,
        task_id,
    );
    Ok(prompt)
}
