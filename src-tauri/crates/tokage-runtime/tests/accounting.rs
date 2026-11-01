#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use native::{prepared, settled};
use tokage_core::UsageSource;
use tokage_runtime::{accounting, repository::Reader, RunState};
#[tokio::test]
async fn receipt_projection_is_replay_safe_frozen_and_keeps_units_separate() {
    let (dir, _, service, start) = prepared("normal", "git status --porcelain", false).await;
    service.submit(&start).await.unwrap();
    let s = settled(&service).await;
    assert_eq!(s.state, RunState::AwaitingMerge);
    let projection = accounting::project(&s).unwrap();
    assert_eq!(projection.receipts.len(), 2);
    assert!(projection.receipts.iter().any(|r| r.phase == "REVIEW"));
    assert!(projection.receipts.iter().any(|r| r.phase == "EXECUTION"));
    assert!(projection.receipts.iter().all(|r| r.price_basis.is_some()));
    assert!(s.turns.values().all(|t| t.accounting_phase.is_some()));
    let mut reader = Reader::open(&dir.path().join("db")).unwrap();
    for _ in 0..2 {
        let read = reader.snapshot(&s.run_id).unwrap().unwrap();
        assert_eq!(accounting::project(&read).unwrap(), projection);
    }
    let mut changed = s.clone();
    changed.config_hash = "new-config".into();
    changed.config.models.clear();
    assert_eq!(accounting::project(&changed).unwrap(), projection); // no repricing on read
    let id = changed.turns.keys().next().unwrap().clone();
    let turn = changed.turns.get_mut(&id).unwrap();
    turn.price_basis = None;
    let usage = turn.usage.as_mut().unwrap();
    usage.source = UsageSource::Estimated;
    usage.cost_usd = Some(0.4);
    usage.credits = Some(12.);
    usage.premium_requests = Some(2.);
    turn.cost_usd = Some(0.4);
    let p = accounting::project(&changed).unwrap();
    let r = p
        .receipts
        .iter()
        .find(|r| r.turn_id.as_ref() == Some(&id))
        .unwrap();
    assert_eq!(r.reported_usd, None);
    assert_eq!(r.estimated_usd, Some(0.4));
    assert_eq!(r.usage.as_ref().unwrap().credits, Some(12.));
    let turn = changed.turns.get_mut(&id).unwrap();
    turn.usage.as_mut().unwrap().source = UsageSource::Structured;
    turn.cost_usd = Some(0.);
    let p = accounting::project(&changed).unwrap();
    let r = p
        .receipts
        .iter()
        .find(|r| r.turn_id.as_ref() == Some(&id))
        .unwrap();
    assert_eq!(r.reported_usd, Some(0.));
    assert_eq!(r.estimated_usd, None);
    let turn = changed.turns.get_mut(&id).unwrap();
    turn.usage = None;
    turn.cost_usd = None;
    let p = accounting::project(&changed).unwrap();
    let r = p
        .receipts
        .iter()
        .find(|r| r.turn_id.as_ref() == Some(&id))
        .unwrap();
    assert_eq!(r.reported_usd, None);
    assert_eq!(r.estimated_usd, None);
}
