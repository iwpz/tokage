//! Child process groups (docs/09 §2.3). npm-installed CLIs run as a `node` wrapper that
//! spawns shells and compilers; killing only the direct child leaves those running.

use std::time::Duration;

/// Puts the child in its own process group so the whole tree can be signalled.
pub fn isolate(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(not(unix))]
    let _ = cmd;
}

/// Terminates the child's process group when dropped: on normal exit (leftover
/// background processes) and when the turn is aborted.
pub struct Group(#[allow(dead_code)] Option<u32>);

impl Group {
    pub fn new(child: &tokio::process::Child) -> Self {
        Self(child.id())
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.and_then(|p| i32::try_from(p).ok()) {
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
        #[cfg(not(unix))]
        let _ = Duration::ZERO;
    }
}
