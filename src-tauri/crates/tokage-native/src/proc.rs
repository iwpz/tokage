//! Child process groups and process hygiene (docs/09 §2.3, §3, R3).
//!
//! Features:
//! - Puts child processes into their own process groups (Unix) or Job Objects (Windows)
//! - Sets child scheduling priority to `nice +5` / `BELOW_NORMAL_PRIORITY_CLASS` (docs/09 §3)
//! - Persists active process groups in `<run_dir>/children.json`
//! - Automatically cleans up leftover processes from crashes / abnormal Tokage exits on startup

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

static RUN_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);
static CHILDREN_LOCK: Mutex<()> = Mutex::new(());
static CRASH_DIRS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Sets the base run directory where `children.json` will be stored.
pub fn set_run_dir(dir: &Path) {
    let mut lock = RUN_DIR.write().unwrap();
    *lock = Some(dir.to_path_buf());
}

/// Returns the current configured run directory, if any.
pub fn get_run_dir() -> Option<PathBuf> {
    if let Some(directory)=scoped_run_directory(){return Some(directory);}
    RUN_DIR.read().unwrap().clone()
}

/// Simulates a crash condition for a specific run directory during testing.
pub fn simulate_crash_for_dir(dir: &Path, enabled: bool) {
    let mut lock = CRASH_DIRS.lock().unwrap();
    if enabled {
        if !lock.contains(&dir.to_path_buf()) {
            lock.push(dir.to_path_buf());
        }
    } else {
        lock.retain(|d| d != dir);
    }
}

/// Simulates a crash condition for the globally configured run directory.
pub fn simulate_crash(enabled: bool) {
    if let Some(dir) = get_run_dir() {
        simulate_crash_for_dir(&dir, enabled);
    }
}

/// Returns whether crash simulation is currently active for the given directory.
pub fn is_crash_simulated_for(dir: Option<&Path>) -> bool {
    let Some(d) = dir else { return false; };
    let lock = CRASH_DIRS.lock().unwrap();
    lock.iter().any(|cd| cd == d)
}

/// A serialized entry for an active child process group in `children.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ChildRecord {
    pub pgid: u32,
    pub pid: u32,
    pub started_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

fn read_records(path: &Path) -> Vec<ChildRecord> {
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn write_records(path: &Path, records: &[ChildRecord]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if records.is_empty() {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        return Ok(());
    }
    let data = serde_json::to_string_pretty(records)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), ts));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Registers a spawned child process into `run/children.json`.
pub fn register_child(run_dir: &Path, pid: u32, pgid: u32, command: Option<String>) {
    let _guard = CHILDREN_LOCK.lock().unwrap();
    let file = run_dir.join("children.json");
    let mut records = read_records(&file);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if !records.iter().any(|r| r.pid == pid && r.pgid == pgid) {
        records.push(ChildRecord {
            pgid,
            pid,
            started_at: now,
            command,
        });
        if let Err(e) = write_records(&file, &records) {
            tracing::warn!("Failed to write children.json: {e}");
        }
    }
}

/// Unregisters a terminated child process from `run/children.json`.
pub fn unregister_child(run_dir: &Path, pgid: u32) {
    let _guard = CHILDREN_LOCK.lock().unwrap();
    let file = run_dir.join("children.json");
    if !file.exists() {
        return;
    }
    let mut records = read_records(&file);
    records.retain(|r| r.pgid != pgid && r.pid != pgid);
    if let Err(e) = write_records(&file, &records) {
        tracing::warn!("Failed to update children.json: {e}");
    }
}

/// Scans `run/children.json` for lingering child process groups left behind by abnormal
/// exits / crashes, terminates them, and deletes the record file (docs/09 §2.3, R3).
pub fn cleanup_leftovers(run_dir: &Path) -> Vec<u32> {
    let _guard = CHILDREN_LOCK.lock().unwrap();
    let file = run_dir.join("children.json");
    if !file.exists() {
        return Vec::new();
    }
    let records: Vec<ChildRecord> = match std::fs::read(&file).ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok()) {
        Some(records) => records,
        None => { tracing::error!("Invalid process registry; preserving it for manual recovery"); return Vec::new(); }
    };
    if records.iter().any(|record| record.pid < 2 || record.pgid < 2 || record.pid > i32::MAX as u32 || record.pgid > i32::MAX as u32) {
        tracing::error!("Invalid process identities; preserving registry for manual recovery");
        return Vec::new();
    }
    let mut cleaned = Vec::new();

    #[cfg(unix)]
    {
        for r in &records {
            let pgid = r.pgid as i32;
            let pid = r.pid as i32;
            let is_alive = unsafe { libc::killpg(pgid, 0) == 0 || libc::kill(pid, 0) == 0 };
            if is_alive {
                tracing::warn!(
                    "Reaping leftover process group {} (pid {}) from previous run",
                    r.pgid,
                    r.pid
                );
                unsafe {
                    libc::killpg(pgid, libc::SIGTERM);
                    libc::kill(pid, libc::SIGTERM);
                }
                cleaned.push(r.pgid);
            }
        }

        if !cleaned.is_empty() {
            // Wait up to 500ms for SIGTERM termination
            for _ in 0..10 {
                let any_alive = cleaned.iter().any(|&pgid| unsafe { libc::killpg(pgid as i32, 0) == 0 });
                if !any_alive {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            // Escalate to SIGKILL for any that are still alive
            for &pgid in &cleaned {
                let pgid_i32 = pgid as i32;
                unsafe {
                    if libc::killpg(pgid_i32, 0) == 0 {
                        libc::killpg(pgid_i32, libc::SIGKILL);
                    }
                    if libc::kill(pgid_i32, 0) == 0 {
                        libc::kill(pgid_i32, libc::SIGKILL);
                    }
                }
            }
        }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::*;
        use windows_sys::Win32::System::Threading::*;
        for r in &records {
            unsafe {
                let handle = OpenProcess(PROCESS_TERMINATE, 0, r.pid);
                if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                    TerminateProcess(handle, 1);
                    CloseHandle(handle);
                    cleaned.push(r.pid);
                }
            }
        }
    }

    let _ = std::fs::remove_file(&file);
    cleaned
}

/// Puts the child in its own process group so the whole tree can be signalled,
/// and lowers its priority to nice +5 (docs/09 §2.3, §3) so background CLI runs
/// do not preempt foreground interactive tasks.
pub fn isolate(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    {
        cmd.process_group(0);
        unsafe {
            cmd.pre_exec(|| {
                // Lower scheduling priority by 5 (nice +5, docs/09 §3).
                libc::nice(5);
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;
        cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS);
    }
}

/// Windows Job Object wrapper (docs/09 §2.3, P3.m5c).
#[cfg(windows)]
pub struct WinJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for WinJob {}
#[cfg(windows)]
unsafe impl Sync for WinJob {}

#[cfg(windows)]
impl WinJob {
    pub fn create() -> Option<Self> {
        use windows_sys::Win32::Foundation::*;
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                CloseHandle(handle);
                return None;
            }
            Some(Self { handle })
        }
    }

    pub fn assign(&self, raw_handle: windows_sys::Win32::Foundation::HANDLE) -> bool {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        unsafe {
            AssignProcessToJobObject(self.handle, raw_handle) != 0
        }
    }

    pub fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        unsafe {
            TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for WinJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

/// Terminates the child's process group when dropped: on normal exit (leftover
/// background processes) and when the turn is aborted.
pub struct Group {
    pid: Option<u32>,
    #[cfg(unix)]
    pgid: Option<u32>,
    #[cfg(windows)]
    job: Option<WinJob>,
    run_dir: Option<PathBuf>,
    disarmed: bool,
    killed: AtomicBool,
}

impl Group {
    pub fn new(child: &tokio::process::Child) -> Self {
        Self::with_dir_cmd(child, get_run_dir(), None)
    }

    pub fn with_dir(child: &tokio::process::Child, run_dir: Option<PathBuf>) -> Self {
        Self::with_dir_cmd(child, run_dir, None)
    }

    pub fn with_dir_cmd(
        child: &tokio::process::Child,
        run_dir: Option<PathBuf>,
        cmd_name: Option<String>,
    ) -> Self {
        let pid = child.id();
        #[cfg(unix)]
        let pgid = pid;

        #[cfg(windows)]
        let job = {
            if let Some(j) = WinJob::create() {
                if let Some(raw) = child.raw_handle() {
                    j.assign(raw as _);
                }
                Some(j)
            } else {
                None
            }
        };

        if let (Some(dir), Some(id)) = (run_dir.as_deref(), pid) {
            register_child(dir, id, id, cmd_name);
        }

        Self {
            pid,
            #[cfg(unix)]
            pgid,
            #[cfg(windows)]
            job,
            run_dir,
            disarmed: false,
            killed: AtomicBool::new(false),
        }
    }

    pub fn id(&self) -> Option<u32> {
        self.pid
    }

    /// Disarms the group drop handler to simulate an abnormal Tokage exit/crash in tests.
    pub fn disarm(&mut self) {
        self.disarmed = true;
    }

    /// Asynchronously terminates the process group (SIGTERM followed by SIGKILL).
    pub async fn kill(&self) {
        if self.disarmed || is_crash_simulated_for(self.run_dir.as_deref()) {
            return;
        }
        self.killed.store(true, Ordering::SeqCst);

        #[cfg(unix)]
        if let Some(pid) = self.pgid.and_then(|p| i32::try_from(p).ok()) {
            if unsafe { libc::killpg(pid, libc::SIGTERM) } == 0 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                unsafe {
                    if libc::killpg(pid, 0) == 0 { libc::killpg(pid, libc::SIGKILL); }
                }
            }
        }

        #[cfg(windows)]
        if let Some(ref job) = self.job {
            job.terminate();
        }

        if let (Some(dir), Some(pgid)) = (self.run_dir.as_deref(), self.pid) {
            #[cfg(unix)] let confirmed=crate::recovery::process_identity_idle(pgid,pgid).unwrap_or(false);
            #[cfg(not(unix))] let confirmed=true;
            if confirmed {unregister_child(dir, pgid);}
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if self.disarmed || is_crash_simulated_for(self.run_dir.as_deref()) {
            return;
        }

        if let (Some(dir), Some(pgid)) = (self.run_dir.as_deref(), self.pid) {
            #[cfg(unix)] let confirmed=crate::recovery::process_identity_idle(pgid,pgid).unwrap_or(false);
            #[cfg(not(unix))] let confirmed=true;
            if confirmed {unregister_child(dir, pgid);}
        }

        let already_killed = self.killed.swap(true, Ordering::SeqCst);

        #[cfg(unix)]
        if let Some(pid) = self.pgid.and_then(|p| i32::try_from(p).ok()) {
            if !already_killed {
                // SAFETY: plain syscalls on a process group id we created; errors are ignored.
                unsafe {
                    libc::killpg(pid, libc::SIGTERM);
                }
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(3));
                    unsafe {
                        if libc::killpg(pid, 0) == 0 {
                            libc::killpg(pid, libc::SIGKILL);
                        }
                    }
                });
            }
        }

        #[cfg(windows)]
        if let Some(ref job) = self.job {
            if !already_killed {
                job.terminate();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_registration_and_unregister_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let run_dir = temp.path().join("run");
        let children_file = run_dir.join("children.json");

        assert!(!children_file.exists());
        register_child(&run_dir, 1001, 1001, Some("test-cli".into()));
        assert!(children_file.exists());

        let recs = read_records(&children_file);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].pid, 1001);
        assert_eq!(recs[0].pgid, 1001);
        assert_eq!(recs[0].command.as_deref(), Some("test-cli"));

        register_child(&run_dir, 1002, 1002, Some("other-cli".into()));
        let recs = read_records(&children_file);
        assert_eq!(recs.len(), 2);

        unregister_child(&run_dir, 1001);
        let recs = read_records(&children_file);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].pid, 1002);

        unregister_child(&run_dir, 1002);
        assert!(!children_file.exists());
    }

    #[tokio::test]
    async fn isolate_sets_nice_plus_five() {
        #[cfg(unix)]
        {
            let mut cmd = tokio::process::Command::new("/bin/sh");
            cmd.args(["-c", "python3 -c 'import os; print(os.getpriority(os.PRIO_PROCESS, 0))' 2>/dev/null || nice"]);
            isolate(&mut cmd);
            let out = cmd.output().await.expect("execute command");
            assert!(out.status.success());
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            assert_eq!(stdout, "5", "process priority should be set to nice +5");
        }
    }

    #[tokio::test]
    async fn isolate_sets_distinct_process_group() {
        #[cfg(unix)]
        {
            let parent_pgrp = unsafe { libc::getpgrp() };
            let mut cmd = tokio::process::Command::new("/bin/sh");
            cmd.args(["-c", "python3 -c 'import os; print(os.getpgrp())' 2>/dev/null || ps -o pgid= -p $$"]);
            isolate(&mut cmd);
            let out = cmd.output().await.expect("execute command");
            assert!(out.status.success());
            let child_pgid: i32 = String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse()
                .expect("parse child pgid");
            assert_ne!(child_pgid, parent_pgrp, "child must be in its own process group");
        }
    }

    #[tokio::test]
    async fn cleanup_leftovers_terminates_orphaned_process_group() {
        let temp = tempfile::tempdir().unwrap();
        let run_dir = temp.path().join("run");

        #[cfg(unix)]
        {
            let mut cmd = tokio::process::Command::new("/bin/sh");
            cmd.args(["-c", "sleep 60"]);
            isolate(&mut cmd);
            let mut child = cmd.spawn().expect("spawn child");
            let pid = child.id().expect("pid");

            // Register in children.json
            register_child(&run_dir, pid, pid, Some("sleep".into()));
            assert!(run_dir.join("children.json").exists());

            // Process is currently alive
            assert_eq!(unsafe { libc::kill(pid as i32, 0) }, 0);

            // Cleanup leftovers
            let cleaned = cleanup_leftovers(&run_dir);
            assert!(cleaned.contains(&pid));

            // Wait for direct child to exit upon receiving the signal
            let mut exited = false;
            for _ in 0..50 {
                if let Ok(Some(_status)) = child.try_wait() {
                    exited = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(exited, "child process must have been terminated");
            assert!(!run_dir.join("children.json").exists(), "children.json must be removed");
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "internal m2d subprocess helper; invoked and checked by m2d_sigkill_host_reaps_registered_tree"]
    async fn m2d_crash_host_helper() {
        let dir = PathBuf::from(std::env::var("TOKAGE_M2D_CRASH_FIXTURE").unwrap());
        let mut cmd = tokio::process::Command::new("/bin/sh");
        cmd.current_dir(&dir).args(["-c", "trap '' TERM; sh -c 'trap \"\" TERM; echo $$ > grandchild.pid; exec sleep 60' & echo $$ > parent.pid; wait"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        isolate(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let _group = Group::with_dir_cmd(&child, Some(dir.join("run")), Some("m2d-isolated-fixture".into()));
        std::fs::write(dir.join("registered"), "ready").unwrap();
        let _ = child.wait().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn m2d_sigkill_host_reaps_registered_tree() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["proc::tests::m2d_crash_host_helper", "--ignored", "--exact"])
            .env("TOKAGE_M2D_CRASH_FIXTURE", dir.path())
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .kill_on_drop(true).spawn().unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !dir.path().join("registered").exists() || !dir.path().join("grandchild.pid").exists() {
                assert!(host.try_wait().unwrap().is_none(), "helper exited before registration");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        let parent: i32 = std::fs::read_to_string(dir.path().join("parent.pid")).unwrap().trim().parse().unwrap();
        let grandchild: i32 = std::fs::read_to_string(dir.path().join("grandchild.pid")).unwrap().trim().parse().unwrap();
        // Real SIGKILL: no destructor runs in the killed host. Children survive until startup cleanup.
        host.kill().await.unwrap();
        assert_eq!(unsafe { libc::kill(grandchild, 0) }, 0);
        assert!(cleanup_leftovers(&dir.path().join("run")).contains(&(parent as u32)));
        for pid in [parent, grandchild] {
            for _ in 0..100 {
                if unsafe { libc::kill(pid, 0) } != 0 { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "orphan {pid} survived cleanup");
        }
        assert!(!dir.path().join("run/children.json").exists());
        assert!(cleanup_leftovers(&dir.path().join("run")).is_empty());
    }

    #[test]
    fn m2e_invalid_startup_registry_is_preserved_and_recovery_stays_blocked() {
        let dir = tempfile::tempdir().unwrap();
        for contents in ["{partial".to_string(), serde_json::to_string(&vec![ChildRecord { pid: 0, pgid: 0, started_at: 0, command: None }]).unwrap(), serde_json::to_string(&vec![ChildRecord { pid: u32::MAX, pgid: u32::MAX, started_at: 0, command: None }]).unwrap()] {
            let file = dir.path().join("children.json");
            std::fs::write(&file, &contents).unwrap();
            assert!(cleanup_leftovers(dir.path()).is_empty());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), contents);
            assert!(crate::recovery::runtime_idle(Some(dir.path())).is_err());
        }
    }

}

// Ordinary service context, scoped to this task; never changes the desktop global registry.
tokio::task_local! {static SCOPED_RUN_DIR:PathBuf;}
pub async fn in_run_directory<F:std::future::Future>(directory:PathBuf,future:F)->F::Output {
    SCOPED_RUN_DIR.scope(directory,future).await
}
pub fn scoped_run_directory()->Option<PathBuf>{SCOPED_RUN_DIR.try_with(Clone::clone).ok()}
/// Intent survives cancellation and owner death. Missing PID is an unresolved spawn window.
pub struct PendingOperation {path:PathBuf}
impl PendingOperation {
    pub fn prepare(directory:&Path)->std::io::Result<Self>{
        std::fs::create_dir_all(directory)?;
        crate::recovery::reconcile_pending_operations(directory)?;
        let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let path=directory.join(format!("pending-{}-{nonce}.json",std::process::id()));
        let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(&path)?;
        use std::io::Write;file.write_all(b"null")?;file.sync_all()?;
        #[cfg(unix)]std::fs::File::open(directory)?.sync_all()?;
        Ok(Self{path})
    }
    pub fn spawned(&self,pid:u32)->std::io::Result<()>{
        use std::io::Write;
        let mut file=std::fs::OpenOptions::new().write(true).truncate(true).open(&self.path)?;
        serde_json::to_writer(&mut file,&serde_json::json!({"pid":pid,"pgid":pid}))?;
        file.flush()?;file.sync_all()
    }
    pub fn unspawned(self)->std::io::Result<()>{std::fs::remove_file(self.path)}
    /// A reaped leader does not prove its descendants have exited. Bound the OS observation
    /// after termination without ever treating a timeout as confirmed cleanup.
    pub async fn wait_completed(self)->std::io::Result<()> {
        let value:serde_json::Value=serde_json::from_slice(&std::fs::read(&self.path)?).map_err(std::io::Error::other)?;
        let number=|key:&str|value[key].as_u64().and_then(|n|u32::try_from(n).ok()).ok_or_else(||std::io::Error::other("Operation PID unknown"));
        let (pid,pgid)=(number("pid")?,number("pgid")?);
        let deadline=tokio::time::Instant::now()+Duration::from_secs(2);
        loop {
            if crate::recovery::process_identity_idle(pid,pgid)? {return self.completed();}
            if tokio::time::Instant::now()>=deadline {return Err(std::io::Error::other("Native Git process cleanup unconfirmed"));}
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    pub fn completed(self)->std::io::Result<()>{
        let value:serde_json::Value=serde_json::from_slice(&std::fs::read(&self.path)?).map_err(std::io::Error::other)?;
        let number=|key:&str|value[key].as_u64().and_then(|n|u32::try_from(n).ok()).ok_or_else(||std::io::Error::other("Operation PID unknown"));
        if !crate::recovery::process_identity_idle(number("pid")?,number("pgid")?)?{return Err(std::io::Error::other("Native Git process cleanup unconfirmed"));}
        std::fs::remove_file(&self.path)?;
        #[cfg(unix)]std::fs::File::open(self.path.parent().unwrap())?.sync_all()?;
        Ok(())
    }
}
#[cfg(test)]
mod runtime_registry_tests {
    use super::*;
    #[tokio::test]
    async fn runtime_registry_scope_preserves_global_and_unknown_spawn_is_fenced(){
        let dir=tempfile::tempdir().unwrap();let previous=get_run_dir();
        in_run_directory(dir.path().to_path_buf(),async{assert_eq!(get_run_dir().as_deref(),Some(dir.path()));let _pending=PendingOperation::prepare(dir.path()).unwrap();assert!(crate::recovery::reconcile_pending_operations(dir.path()).is_err());}).await;
        assert_eq!(get_run_dir(),previous);assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(),1);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn runtime_registry_live_operation_is_not_released_until_process_exit(){
        let dir=tempfile::tempdir().unwrap();let pending=PendingOperation::prepare(dir.path()).unwrap();
        let mut command=tokio::process::Command::new("/bin/sleep");command.arg("30").kill_on_drop(true);isolate(&mut command);let mut child=command.spawn().unwrap();pending.spawned(child.id().unwrap()).unwrap();
        assert!(crate::recovery::reconcile_pending_operations(dir.path()).is_err());assert!(pending.completed().is_err());
        child.kill().await.unwrap();child.wait().await.unwrap();crate::recovery::reconcile_pending_operations(dir.path()).unwrap();assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(),0);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn runtime_registry_completion_waits_for_actual_process_exit(){
        let dir=tempfile::tempdir().unwrap();let pending=PendingOperation::prepare(dir.path()).unwrap();
        let mut command=tokio::process::Command::new("/bin/sleep");command.arg("30").kill_on_drop(true);isolate(&mut command);let mut child=command.spawn().unwrap();pending.spawned(child.id().unwrap()).unwrap();
        let wait=pending.wait_completed();tokio::pin!(wait);
        tokio::select! {result=&mut wait=>panic!("cleanup returned before process exit: {result:?}"),_=tokio::time::sleep(Duration::from_millis(25))=>{}}
        child.kill().await.unwrap();child.wait().await.unwrap();wait.await.unwrap();assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(),0);
    }

}
