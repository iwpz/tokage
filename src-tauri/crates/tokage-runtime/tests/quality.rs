#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use native::{prepared, settled};
use tokage_orchestration::TaskState;
use tokage_runtime::{quality, turn::TurnStatus, *};
fn result(s: &Snapshot) -> String {
    quality::observations(s).unwrap()[0].outcome.clone()
}
#[tokio::test]
async fn accepted_work_proves_first_pass_and_retries_never_become_new_first_passes() {
    let (dir, _cwd, service, start) = prepared("normal", "git status --porcelain", false).await;
    service.submit(&start).await.unwrap();
    let s = settled(&service).await;
    assert_eq!(s.state, RunState::AwaitingMerge);
    assert_eq!(result(&s), "FIRST_PASS");
    let rows = quality::observations(&s).unwrap();
    assert!(rows[0].observed_at.is_some());
    assert_eq!(quality::read(&dir.path().join("db")).unwrap(), rows);
    assert_eq!(quality::read(&dir.path().join("db")).unwrap(), rows); // observer while owner lives never increments anything
    assert_eq!(rows[0].run_state, RunState::AwaitingMerge); // task accepted != delivery
    let a = serde_json::to_value(quality::learned(&rows)).unwrap();
    assert_eq!(
        a,
        serde_json::to_value(quality::learned(&[rows.clone(), rows.clone()].concat())).unwrap()
    );
    assert_eq!(quality::learned(&rows)[0].alpha, 1.0);

    let mut retry = s.clone();
    let current = retry.execution.as_ref().unwrap().tasks["T1"].attempts[0].clone();
    let mut prior = retry.turns[&current].clone();
    prior.id = "failed-before".into();
    prior.status = TurnStatus::Failed;
    prior.error = Some("gate failed".into());
    prior.thread_id = Some("same-thread".into());
    retry.turns.get_mut(&current).unwrap().thread_id = prior.thread_id.clone();
    retry.turns.insert(prior.id.clone(), prior);
    retry
        .execution
        .as_mut()
        .unwrap()
        .tasks
        .get_mut("T1")
        .unwrap()
        .attempts
        .insert(0, "failed-before".into());
    assert_eq!(result(&retry), "FIXED_IN_WINDOW");
    assert_eq!(
        quality::learned(&quality::observations(&retry).unwrap())[0].alpha,
        0.0
    );
    assert_eq!(
        quality::learned(&quality::observations(&retry).unwrap())[0].beta,
        1.0
    );
    retry.turns.get_mut(&current).unwrap().thread_id = Some("new-thread".into());
    assert_eq!(result(&retry), "RETRIED");
    retry
        .turns
        .get_mut(&current)
        .unwrap()
        .requested
        .requested_model = "replacement-model".into();
    assert_eq!(result(&retry), "REASSIGNED");
    assert_ne!(
        quality::observations(&retry).unwrap()[0].model,
        "replacement-model"
    ); // original executor denominator

    let mut withdrawn = retry.clone();
    withdrawn.plan.as_mut().unwrap().plan.tasks.clear();
    let removed = quality::observations(&withdrawn).unwrap();
    assert_eq!(removed[0].outcome, "REMOVED");
    assert!(!removed[0].learning_eligible);
    assert!(quality::learned(&removed).is_empty());
    let mut reintroduced = retry.clone();
    reintroduced
        .execution
        .as_mut()
        .unwrap()
        .tasks
        .get_mut("T1")
        .unwrap()
        .attempts = vec![current.clone()];
    let replay = quality::observations(&reintroduced).unwrap();
    assert_eq!(replay[0].outcome, "RETRIED");
    assert_eq!(replay[0].attempt_ids.len(), 2);
    assert!(!replay[0].learning_eligible);

    let mut missing = s.clone();
    missing
        .work
        .as_mut()
        .unwrap()
        .attempts
        .get_mut(&current)
        .unwrap()
        .gates
        .clear();
    assert_eq!(result(&missing), "UNKNOWN");
    assert!(quality::learned(&quality::observations(&missing).unwrap()).is_empty());
    let mut late = s.clone();
    late.turns.get_mut(&current).unwrap().error = Some("late failure".into());
    assert_eq!(result(&late), "UNKNOWN");
    let mut historical = s.clone();
    historical
        .work
        .as_mut()
        .unwrap()
        .attempts
        .get_mut(&current)
        .unwrap()
        .accepted_at = None;
    assert_eq!(
        quality::observations(&historical).unwrap()[0].observed_at,
        None
    );

    let mut stopped = s.clone();
    stopped.state = RunState::Stopped;
    stopped
        .execution
        .as_mut()
        .unwrap()
        .tasks
        .get_mut("T1")
        .unwrap()
        .state = TaskState::Blocked;
    stopped.turns.get_mut(&current).unwrap().status = TurnStatus::Stopped;
    assert_eq!(result(&stopped), "ABANDONED");
    assert!(quality::learned(&quality::observations(&stopped).unwrap()).is_empty()); // user cancel is not model failure
    stopped.turns.get_mut(&current).unwrap().status = TurnStatus::Failed;
    assert_eq!(
        quality::learned(&quality::observations(&stopped).unwrap())[0].beta,
        1.0
    );
    stopped.state = RunState::Blocked;
    assert_eq!(result(&stopped), "BLOCKED");
    assert!(quality::learned(&quality::observations(&stopped).unwrap()).is_empty());
}
#[test]
fn read_does_not_create_data_or_an_owner() {
    let d = tempfile::tempdir().unwrap();
    let absent = d.path().join("not-created");
    assert!(quality::read(&absent).unwrap().is_empty());
    assert!(!absent.exists());
}
