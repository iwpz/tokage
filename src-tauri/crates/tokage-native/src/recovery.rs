//! Read-only recovery evidence and durable at-most-once launch claims.
use std::{io::{self, Write}, path::{Path, PathBuf}};
use sha2::{Digest, Sha256};

pub fn receipt_path(root: &Path, turn_id: &str) -> PathBuf {
    root.join(format!("{:x}.json", Sha256::digest(turn_id.as_bytes())))
}

pub fn claim_launch(root: &Path, turn_id: &str, metadata: &serde_json::Value) -> io::Result<()> {
    std::fs::create_dir_all(root)?;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(receipt_path(root, turn_id))
        .map_err(|e| io::Error::new(e.kind(), format!("Turn already submitted or launch receipt unavailable: {e}")))?;
    file.write_all(serde_json::to_vec(metadata).map_err(io::Error::other)?.as_slice())?;
    file.sync_all()?;
    #[cfg(unix)] std::fs::File::open(root)?.sync_all()?;
    Ok(())
}

pub fn verify_launch(root: &Path, turn_id: &str, cwd: &str) -> io::Result<bool> {
    let bytes = match std::fs::read(receipt_path(root, turn_id)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if value["turnId"].as_str() != Some(turn_id) || value["cwd"].as_str() != Some(cwd) {
        return Err(io::Error::other("Native launch receipt identity mismatch"));
    }
    Ok(true)
}

pub fn runtime_idle(run_dir: Option<&Path>) -> io::Result<bool> {
    let Some(root) = run_dir else { return Err(io::Error::other("Process registry directory unavailable")); };
    let bytes = match std::fs::read(root.join("children.json")) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e),
    };
    let records: Vec<crate::proc::ChildRecord> = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    #[cfg(unix)] {
        for record in records {
            if record.pgid < 2 || record.pid < 2 || record.pid > i32::MAX as u32 || record.pgid > i32::MAX as u32 { return Err(io::Error::other("Invalid process identity")); }
            if unsafe { libc::killpg(record.pgid as i32, 0) == 0 || libc::kill(record.pid as i32, 0) == 0 }
                || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) { return Ok(false); }
        }
        Ok(true)
    }
    #[cfg(not(unix))] { Ok(records.is_empty()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn m2e_duplicate_launch_claims_and_corrupt_registry_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = serde_json::json!({"turnId":"../../same-turn", "task":"T1", "cwd":"/fixture"});
        claim_launch(dir.path(), "../../same-turn", &metadata).unwrap();
        assert!(claim_launch(dir.path(), "../../same-turn", &metadata).is_err());
        assert!(verify_launch(dir.path(), "../../same-turn", "/fixture").unwrap());
        assert!(verify_launch(dir.path(), "../../same-turn", "/other-project").is_err());
        std::fs::write(receipt_path(dir.path(), "corrupt"), "{partial").unwrap();
        assert!(verify_launch(dir.path(), "corrupt", "/fixture").is_err());
        assert!(receipt_path(dir.path(), "../../same-turn").starts_with(dir.path()));
        assert!(runtime_idle(Some(dir.path())).unwrap());
        std::fs::write(dir.path().join("children.json"), "{partial").unwrap();
        assert!(runtime_idle(Some(dir.path())).is_err());
        #[cfg(unix)] {
            let records = vec![crate::proc::ChildRecord { pid: std::process::id(), pgid: unsafe { libc::getpgrp() as u32 }, started_at: 0, command: None }];
            std::fs::write(dir.path().join("children.json"), serde_json::to_vec(&records).unwrap()).unwrap();
            assert!(!runtime_idle(Some(dir.path())).unwrap());
        }
    }
}

pub fn process_identity_idle(pid:u32,pgid:u32)->io::Result<bool>{
    if pid<2 || pgid<2 || pid>i32::MAX as u32 || pgid>i32::MAX as u32{return Err(io::Error::other("Invalid process identity"));}
    #[cfg(unix)]{
        for (id,group) in [(pid,false),(pgid,true)]{
            let result=unsafe{if group{libc::killpg(id as i32,0)}else{libc::kill(id as i32,0)}};
            if result==0{return Ok(false);}
            if io::Error::last_os_error().raw_os_error()!=Some(libc::ESRCH){return Ok(false);}
        }
        Ok(true)
    }
    #[cfg(not(unix))]{Ok(false)}
}
/// Pending native operations are cleared only after the recorded process group is absent.
/// A crash between intent and PID capture requires manual reconciliation.
pub fn reconcile_pending_operations(directory:&Path)->io::Result<()> {
    if !directory.exists(){return Ok(());}
    for item in std::fs::read_dir(directory)? {
        let path=item?.path();let name=path.file_name().and_then(|p|p.to_str()).unwrap_or_default();
        if !name.starts_with("pending-") || !name.ends_with(".json"){continue;}
        let value:serde_json::Value=serde_json::from_slice(&std::fs::read(&path)?).map_err(io::Error::other)?;
        let number=|key:&str|value[key].as_u64().and_then(|n|u32::try_from(n).ok()).ok_or_else(||io::Error::other("Native launch PID unknown"));
        if !process_identity_idle(number("pid")?,number("pgid")?)? {return Err(io::Error::other("Native operation process still active"));}
        std::fs::remove_file(path)?;
    }
    Ok(())
}
