#![cfg(unix)]
mod common;
#[path="common/native.rs"]
mod native;
use tokage_runtime::*;
#[tokio::test]
async fn completed_codex_tokens_without_reported_usd_are_persisted_as_exclusions_not_training(){
 let(dir,_,service,start)=native::prepared("normal","git status --porcelain",false).await;
 service.submit(&start).await.unwrap();let s=native::settled(&service).await;assert_eq!(s.state,RunState::AwaitingMerge);
 let before=calibration::read(&dir.path().join("db"),5).unwrap();assert_eq!(before.eligible_samples,0);assert!(before.overrides.is_empty());assert_eq!(before.ignored_reasons.get("REPORTED_USD_UNKNOWN"),Some(&1));
 while service.busy(){tokio::task::yield_now().await;}
 drop(service);
 let read=calibration::read(&dir.path().join("db"),5).unwrap();assert_eq!(serde_json::to_value(read).unwrap(),serde_json::to_value(before).unwrap());
}
