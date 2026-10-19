//! OS-owned advisory locks: closing/crashing releases ownership, without stale lock deletion.
use std::{fs::{File, OpenOptions}, io, path::Path};

pub struct FileLease { _file: File }
impl FileLease {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)] {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path)?;
        #[cfg(unix)] {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        #[cfg(windows)] {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY};
            let mut offset = unsafe { std::mem::zeroed() };
            if unsafe { LockFileEx(file.as_raw_handle() as _, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, &mut offset) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn m2e_lease_child() {
        let Some(path) = std::env::var_os("TOKAGE_M2E_LEASE") else { return; };
        if std::env::var_os("TOKAGE_M2E_HOLD").is_some() {
            let _lease = FileLease::acquire(Path::new(&path)).unwrap();
            std::fs::write(Path::new(&path).with_extension("ready"), "ready").unwrap();
            std::thread::sleep(std::time::Duration::from_secs(60));
        } else { assert!(FileLease::acquire(Path::new(&path)).is_err()); }
    }
    #[test]
    fn m2e_os_lock_excludes_other_process_and_sigkill_releases_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lease");
        let lease = FileLease::acquire(&path).unwrap();
        let child = || {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "file_lease::tests::m2e_lease_child"]).env("TOKAGE_M2E_LEASE", &path)
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            command
        };
        assert!(child().status().unwrap().success());
        drop(lease);
        let mut owner = child().env("TOKAGE_M2E_HOLD", "1").spawn().unwrap();
        for _ in 0..500 {
            if path.with_extension("ready").exists() { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(path.with_extension("ready").exists());
        assert!(FileLease::acquire(&path).is_err());
        owner.kill().unwrap(); owner.wait().unwrap();
        assert!(FileLease::acquire(&path).is_ok());
    }
}
