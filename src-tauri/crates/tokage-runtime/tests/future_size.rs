mod common;
use tokage_runtime::{owned::Runtime,repository::SqliteRepository,service::NativePlanner};
#[test]
fn public_command_future_does_not_inline_large_native_policy_and_recovery_state(){
 let dir=tempfile::tempdir().unwrap();
 let repo=SqliteRepository::open(&dir.path().join("db"),"run").unwrap();
 let planner=NativePlanner::new(tokage_native::env::ShellEnv{vars:Default::default()},&dir.path().join("native")).unwrap();
 let service=Runtime::new(repo,"run",planner).unwrap();let request=common::create();
 let future=service.submit(&request);
 assert!(std::mem::size_of_val(&future)<=4096,"public submit future is {} bytes",std::mem::size_of_val(&future));
 // Creating and dropping a future never submits or launches a native request.
 drop(future);
}
