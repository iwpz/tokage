//! Real mutation / lost-acknowledgement boundaries, with independently verified Git trees.
use super::*;

#[tokio::test]
async fn m2e_committed_and_delivered_retries_preserve_exact_tree_without_duplicate_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo"); std::fs::create_dir(&repo).unwrap();
    let env = ShellEnv { vars: std::env::vars().collect() };
    for args in [vec!["init","-qb","main"], vec!["config","user.name","fixture"], vec!["config","user.email","fixture@example.invalid"]] {
        run(&env, &repo, &args, &[], 10).await.unwrap();
    }
    std::fs::write(repo.join("base"), "keep\n").unwrap();
    run(&env,&repo,&["add","."],&[],10).await.unwrap();
    run(&env,&repo,&["commit","-qm","base"],&[],10).await.unwrap();
    let target = checkout_identity(&env,&repo).await.unwrap();
    let wt = create_worktree(&env,&repo,&temp.path().join("app"),"m2e",None).await.unwrap();
    let root = Path::new(&wt.root);
    std::fs::write(root.join("result.bin"), [0,255,3,4]).unwrap();
    let accepted = scope::snapshot(&env,root).await.unwrap();
    assert!(scope::commit_verified(&env,root,"accepted",&accepted).await.unwrap());
    let committed = rev_parse(&env,root,"HEAD").await.unwrap();
    // Simulated lost commit acknowledgement: re-enter with the original accepted snapshot.
    assert!(!scope::commit_verified(&env,root,"accepted",&accepted).await.unwrap());
    assert_eq!(rev_parse(&env,root,"HEAD").await.unwrap(), committed);
    let tree = rev_parse(&env,root,"HEAD^{tree}").await.unwrap();
    let snapshot = scope::snapshot(&env,root).await.unwrap();
    let patch = temp.path().join("pinned.patch");
    export_patch_file_retry(&env,&repo,&target.head,&committed,&patch,false).await.unwrap();
    let bytes = std::fs::read(&patch).unwrap();
    export_patch_file_retry(&env,&repo,&target.head,&committed,&patch,true).await.unwrap();
    assert_eq!(std::fs::read(&patch).unwrap(), bytes);
    std::fs::write(&patch,b"human edited artifact").unwrap();
    assert!(export_patch_file_retry(&env,&repo,&target.head,&committed,&patch,true).await.is_err());
    assert!(!reconcile_delivery_merge(&env,&repo,&committed,&target).await.unwrap());
    merge_accepted_into_checkout(&env,&repo,&wt.branch,&snapshot,Some(&target)).await.unwrap();
    let merged = rev_parse(&env,&repo,"HEAD").await.unwrap();
    // Simulated lost final delivery acknowledgement: recognize exact parents + merge tree.
    for _ in 0..2 { assert!(reconcile_delivery_merge(&env,&repo,&committed,&target).await.unwrap()); }
    assert_eq!(rev_parse(&env,&repo,"HEAD").await.unwrap(), merged);
    assert_eq!(rev_parse(&env,&repo,"HEAD^{tree}").await.unwrap(), tree);
    assert!(contains_commit(&env,&repo,"main",&committed).await.unwrap());
    remove_worktree_if_head(&env,root,&wt.branch,false,true,Some(&committed)).await.unwrap();
    assert_eq!(rev_parse(&env,&repo,&wt.branch).await.unwrap(), committed);
    assert!(reconcile_delivery_merge(&env,&repo,&committed,&target).await.unwrap());
    if let Some(directory) = std::env::var_os("TOKAGE_M2E_EVIDENCE_DIR") {
        let directory = PathBuf::from(directory); std::fs::create_dir_all(&directory).unwrap();
        let bundle = directory.join("recovered.bundle");
        if bundle.exists() { std::fs::remove_file(&bundle).unwrap(); }
        run(&env,&repo,&["bundle","create",bundle.to_str().unwrap(),"--all"],&[],20).await.unwrap();
        std::fs::write(directory.join("delivery.patch"), &bytes).unwrap();
        std::fs::write(directory.join("git-receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "baseCommit": target.head, "sourceCommit": committed, "deliveredCommit": merged,
            "expectedTree": tree, "retainedBranch": wt.branch,
            "commitReplayCreatedCommit": false, "deliveryRecoveredWithoutNewMerge": true
        })).unwrap()).unwrap();
    }
    run(&env,&repo,&["checkout","-qb","human"],&[],10).await.unwrap();
    assert!(reconcile_delivery_merge(&env,&repo,&committed,&target).await.is_err());
}
