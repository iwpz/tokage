mod common;
use common::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokage_core::{EventKind as NativeEvent, UsageSource};
use tokage_runtime::{
    owned::Runtime,
    repository::SqliteRepository,
    service::{Planner, TurnControl},
    turn::TurnStatus,
    *,
};
use tokio::sync::Notify;
fn done() -> NativeEvent {
    NativeEvent::Done {
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        api_calls: None,
        context_tokens: None,
        cost_usd: None,
        premium_requests: None,
        credits: None,
        source: UsageSource::Structured,
    }
}
struct Backend {
    calls: Arc<AtomicUsize>,
    gate: Arc<Notify>,
    mode: &'static str,
}
impl Planner for Backend {
    async fn generate_observed(
        &self,
        _: &Snapshot,
        _: &str,
        mut control: TurnControl,
    ) -> Result<String> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        control
            .events
            .send(NativeEvent::Thread {
                id: "thread".into(),
            })
            .await
            .unwrap();
        if self.mode == "cleanup-unknown" {
            return Err(Error::CleanupUnknown);
        }
        if self.mode == "approval" || self.mode == "session" && call == 1 {
            control
                .events
                .send(NativeEvent::ApprovalRequest {
                    id: "approval".into(),
                    tool_id: None,
                    command: "cargo check".into(),
                    level: "sensitive".into(),
                    reason: "needs permission".into(),
                    recoverable: Some(true),
                })
                .await
                .unwrap();
            return Err(Error::AwaitingApproval);
        }
        if self.mode != "missing-done" {
            control.events.send(done()).await.unwrap();
            control.events.send(done()).await.unwrap();
        }
        control
            .events
            .send(NativeEvent::Text { text: PLAN.into() })
            .await
            .unwrap();
        tokio::select! {_=self.gate.notified()=>Ok(PLAN.into()),_=control.cancel.changed()=>Err(Error::Cancelled)}
    }
}
async fn setup(
    mode: &'static str,
) -> (
    tempfile::TempDir,
    Runtime<Backend>,
    Arc<AtomicUsize>,
    Arc<Notify>,
) {
    let dir = tempfile::tempdir().unwrap();
    let repo = SqliteRepository::open(dir.path(), "run").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Notify::new());
    let service = Runtime::new(
        repo,
        "run",
        Backend {
            calls: calls.clone(),
            gate: gate.clone(),
            mode,
        },
    )
    .unwrap();
    let mut req = create();
    if mode == "session" {
        if let Command::CreateRun { config, .. } = &mut req.command {
            let m = config.models.get_mut("executor").unwrap();
            config.block_unknown_cost = false;
            m.provider = "CLAUDE_CODE".into();
            m.requested_model = "sonnet".into();
        }
    }
    service.submit(&req).await.unwrap();
    (dir, service, calls, gate)
}
async fn wait_for<P: Planner + 'static>(service: &Runtime<P>, state: RunState) -> Snapshot {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let s = service.snapshot().await.unwrap().unwrap();
            if s.state == state {
                return s;
            }
            if let Some(e) = service.worker_error() {
                panic!("worker: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn durable_stream_replay_usage_and_detached_views_do_not_relaunch() {
    let (_dir, service, calls, gate) = setup("normal").await;
    let s = service.snapshot().await.unwrap().unwrap();
    let req = command(&s, "plan", Command::GeneratePlan);
    let receipt = service.submit(&req).await.unwrap();
    let detached_view = service.clone();
    drop(detached_view);
    // Wait until every observation is durable while the worker is still running.
    let active = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let s = service.snapshot().await.unwrap().unwrap();
            if s.turns.values().any(|t| t.last_sequence == 4) {
                break s;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let turn = active.turns.values().next().unwrap();
    assert!(turn.observed_model.is_none());
    assert_eq!(turn.usage.as_ref().unwrap().input_tokens, 10);
    assert!(turn.usage.as_ref().unwrap().cost_usd.is_none());
    assert_eq!(service.submit(&req).await.unwrap(), receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let first = service.transcript(&turn.id, 0, 2).await.unwrap();
    let rest = service
        .transcript(&turn.id, first.last().unwrap().sequence, 100)
        .await
        .unwrap();
    assert_eq!(first.len() + rest.len(), 4);
    gate.notify_one();
    let final_state = wait_for(&service, RunState::AwaitingApproval).await;
    let t = final_state.turns.values().next().unwrap();
    assert_eq!(t.status, TurnStatus::Completed);
    assert!(t.cleanup_confirmed);
    assert_eq!(service.submit(&req).await.unwrap(), receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn cancellation_after_done_is_stopped_only_after_worker_cleanup() {
    let (_dir, service, calls, _) = setup("normal").await;
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(&s, "plan", Command::GeneratePlan))
        .await
        .unwrap();
    let s = loop {
        let s = service.snapshot().await.unwrap().unwrap();
        if s.turns.values().any(|t| t.last_sequence == 4) {
            break s;
        }
        tokio::task::yield_now().await;
    };
    let cancel = command(&s, "cancel", Command::CancelRun);
    service.submit(&cancel).await.unwrap();
    let stopped = wait_for(&service, RunState::Stopped).await;
    assert_eq!(
        stopped.turns.values().next().unwrap().status,
        TurnStatus::Stopped
    );
    assert!(stopped.plan.is_none());
    let events = service.events(0, 100).await.unwrap();
    assert!(events.iter().any(|e| e.kind == EventKind::CancelRequested));
    assert!(events.iter().any(|e| e.kind == EventKind::Stopping));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    service.submit(&cancel).await.unwrap();
    assert!(matches!(
        service
            .submit(&command(
                &stopped,
                "start",
                Command::StartExecution {
                    confirmation_digest: "old".into()
                }
            ))
            .await,
        Err(Error::Unsupported)
    ));
}
#[tokio::test]
async fn missing_structured_completion_blocks_instead_of_accepting_text() {
    let (_dir, service, _, gate) = setup("missing-done").await;
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(&s, "plan", Command::GeneratePlan))
        .await
        .unwrap();
    gate.notify_one();
    let s = wait_for(&service, RunState::Blocked).await;
    assert!(s.plan.is_none());
    assert_eq!(s.turns.values().next().unwrap().status, TurnStatus::Failed);
}
#[tokio::test]
async fn approval_is_bound_expired_and_once_is_not_upgraded_to_session() {
    let (_dir, service, calls, _) = setup("approval").await;
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(&s, "plan", Command::GeneratePlan))
        .await
        .unwrap();
    let s = wait_for(&service, RunState::AwaitingTurnApproval).await;
    let t = s.turns.values().next().unwrap();
    let a = t.approval.as_ref().unwrap();
    let resolve = |digest: String| Command::ResolveApproval {
        decision: ApprovalDecision::AllowOnce,
        approval_id: a.id.clone(),
        turn_id: t.id.clone(),
        confirmation_digest: digest,
    };
    assert!(service
        .submit(&command(&s, "stale", resolve("stale".into())))
        .await
        .is_err());
    let req = command(&s, "once", resolve(a.confirmation_digest.clone()));
    let receipt = service.submit(&req).await.unwrap();
    let blocked = wait_for(&service, RunState::Blocked).await;
    assert_eq!(service.submit(&req).await.unwrap(), receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(service
        .submit(&command(
            &blocked,
            "late",
            resolve(a.confirmation_digest.clone())
        ))
        .await
        .is_err());
}

#[tokio::test]
async fn scoped_session_approval_creates_one_correlated_continuation() {
    let (_dir, service, calls, gate) = setup("session").await;
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(&s, "plan", Command::GeneratePlan))
        .await
        .unwrap();
    let s = wait_for(&service, RunState::AwaitingTurnApproval).await;
    let t = s.turns.values().next().unwrap();
    let a = t.approval.as_ref().unwrap();
    let req = command(
        &s,
        "session-approval",
        Command::ResolveApproval {
            decision: ApprovalDecision::AllowSession,
            approval_id: a.id.clone(),
            turn_id: t.id.clone(),
            confirmation_digest: a.confirmation_digest.clone(),
        },
    );
    let receipt = service.submit(&req).await.unwrap();
    gate.notify_one();
    let s = wait_for(&service, RunState::AwaitingApproval).await;
    assert_eq!(s.turns.len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(service.submit(&req).await.unwrap(), receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn uncertain_cleanup_remains_blocked_and_cannot_be_restarted() {
    let (_dir, service, calls, _) = setup("cleanup-unknown").await;
    let s = service.snapshot().await.unwrap().unwrap();
    let req = command(&s, "plan", Command::GeneratePlan);
    service.submit(&req).await.unwrap();
    let s = wait_for(&service, RunState::Blocked).await;
    assert_eq!(s.turns.values().next().unwrap().status, TurnStatus::Unknown);
    assert!(!s.turns.values().next().unwrap().cleanup_confirmed);
    service.submit(&req).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn failed_transcript_storage_stops_worker_and_remains_blocked_not_user_cancelled() {
    let (dir, service, calls, _) = setup("normal").await;
    let db = rusqlite::Connection::open(dir.path().join("tokage.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_transcript BEFORE INSERT ON runtime_turn_events BEGIN SELECT RAISE(ABORT,'transcript storage unavailable'); END;").unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let req = command(&s, "plan", Command::GeneratePlan);
    let receipt = service.submit(&req).await.unwrap();
    let blocked = wait_for(&service, RunState::Blocked).await;
    assert!(blocked.plan.is_none());
    let turn = blocked.turns.values().next().unwrap();
    assert!(turn.ended && turn.cleanup_confirmed);
    assert_eq!(turn.status, TurnStatus::Failed);
    assert!(matches!(service.worker_error(), Some(Error::Repository(_))));
    assert_eq!(service.submit(&req).await.unwrap(), receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
