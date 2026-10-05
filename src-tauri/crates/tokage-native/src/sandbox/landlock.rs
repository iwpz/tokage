//! Linux Landlock sandbox profile generation and enforcement (docs/11 §5.1, P2.m7a).
//!
//! Landlock is an unprivileged security module in Linux 5.13+ that restricts
//! filesystem operations for a process and its children using handled access masks.
//! Any handled path beneath rights not explicitly allowed are denied by the kernel (EACCES).

#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

use tokage_core::ProviderId;

use super::{expand_pattern, SandboxConfig, SandboxProfile};

// Landlock syscall numbers on Linux x86_64, aarch64, riscv64.
pub const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
pub const SYS_LANDLOCK_ADD_RULE: libc::c_long = 445;
pub const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;

// Flags and constants.
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;

// Access rights for filesystem (ABI v1).
pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
// ABI v2
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
// ABI v3
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandlockRulesetAttr {
    pub handled_access_fs: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandlockPathBeneathAttr {
    pub allowed_access: u64,
    pub parent_fd: i32,
}

/// Linux Landlock sandbox profile holding allowed paths and access masks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandlockProfile {
    pub ruleset_attr: LandlockRulesetAttr,
    pub read_access: u64,
    pub write_access: u64,
    pub allowed_reads: Vec<PathBuf>,
    pub allowed_writes: Vec<PathBuf>,
}

/// Default standard Linux system directories needed for basic execution (shared libs, tools, /dev).
pub const DEFAULT_SYSTEM_DIRS: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr",
    "/lib",
    "/lib64",
    "/etc",
    "/proc",
    "/sys",
    "/dev",
];

/// Builds a `LandlockProfile` for the given provider and sandbox configuration.
pub fn build_landlock_profile(
    _provider: ProviderId,
    profile: &SandboxProfile,
    config: &SandboxConfig,
) -> LandlockProfile {
    let read_access = LANDLOCK_ACCESS_FS_EXECUTE
        | LANDLOCK_ACCESS_FS_READ_FILE
        | LANDLOCK_ACCESS_FS_READ_DIR;

    let write_access = read_access
        | LANDLOCK_ACCESS_FS_WRITE_FILE
        | LANDLOCK_ACCESS_FS_REMOVE_DIR
        | LANDLOCK_ACCESS_FS_REMOVE_FILE
        | LANDLOCK_ACCESS_FS_MAKE_CHAR
        | LANDLOCK_ACCESS_FS_MAKE_DIR
        | LANDLOCK_ACCESS_FS_MAKE_REG
        | LANDLOCK_ACCESS_FS_MAKE_SOCK
        | LANDLOCK_ACCESS_FS_MAKE_FIFO
        | LANDLOCK_ACCESS_FS_MAKE_BLOCK
        | LANDLOCK_ACCESS_FS_MAKE_SYM
        | LANDLOCK_ACCESS_FS_REFER
        | LANDLOCK_ACCESS_FS_TRUNCATE;

    let handled_access_fs = write_access;

    // Allowed reads: system dirs, project root, and profile-specific read paths.
    let mut allowed_reads = Vec::new();
    for sys in DEFAULT_SYSTEM_DIRS {
        let p = PathBuf::from(sys);
        if !allowed_reads.contains(&p) {
            allowed_reads.push(p);
        }
    }

    if !allowed_reads.contains(&config.project_root) {
        allowed_reads.push(config.project_root.clone());
    }

    for p in &profile.read {
        for exp in expand_pattern(p, profile, config) {
            let clean = clean_path_pattern(&exp);
            let path = PathBuf::from(clean);
            if !allowed_reads.contains(&path) {
                allowed_reads.push(path);
            }
        }
    }

    // Allowed writes: worktree, temp dirs, and profile-specific write paths.
    let mut allowed_writes = Vec::new();
    if !allowed_writes.contains(&config.worktree) {
        allowed_writes.push(config.worktree.clone());
    }
    for t in &config.temp_dirs {
        if !allowed_writes.contains(t) {
            allowed_writes.push(t.clone());
        }
    }
    // Also include /dev/null, /dev/zero, /dev/tty for write data
    for dev in ["/dev/null", "/dev/zero", "/dev/tty"] {
        let p = PathBuf::from(dev);
        if !allowed_writes.contains(&p) {
            allowed_writes.push(p);
        }
    }

    for p in &profile.write {
        for exp in expand_pattern(p, profile, config) {
            let clean = clean_path_pattern(&exp);
            let path = PathBuf::from(clean);
            if !allowed_writes.contains(&path) {
                allowed_writes.push(path);
            }
        }
    }

    LandlockProfile {
        ruleset_attr: LandlockRulesetAttr { handled_access_fs },
        read_access,
        write_access,
        allowed_reads,
        allowed_writes,
    }
}

/// Trims glob wildcards from a path pattern for filesystem rules.
fn clean_path_pattern(p: &str) -> &str {
    p.trim_end_matches("/**")
        .trim_end_matches("/*")
        .trim_end_matches('*')
}

/// Checks whether Landlock is supported by the running Linux kernel.
pub fn probe_landlock_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        let abi = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                std::ptr::null::<()>(),
                0usize,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        };
        abi >= 1
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Applies the `LandlockProfile` in the current child process (e.g. within `pre_exec`).
pub fn apply_landlock(profile: &LandlockProfile) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // 1. Query Landlock ABI version
        let abi_version = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                std::ptr::null::<()>(),
                0usize,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        };
        if abi_version < 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Landlock is not supported by current Linux kernel",
            ));
        }

        // 2. Adjust handled_access_fs for older ABI versions
        let mut handled = profile.ruleset_attr.handled_access_fs;
        if abi_version < 2 {
            handled &= !LANDLOCK_ACCESS_FS_REFER;
        }
        if abi_version < 3 {
            handled &= !LANDLOCK_ACCESS_FS_TRUNCATE;
        }

        let attr = LandlockRulesetAttr {
            handled_access_fs: handled,
        };

        let ruleset_fd = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                &attr as *const LandlockRulesetAttr,
                std::mem::size_of::<LandlockRulesetAttr>(),
                0u32,
            )
        };
        if ruleset_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let ruleset_fd = ruleset_fd as i32;

        let add_path_rule = |path: &Path, access: u64| {
            use std::os::unix::ffi::OsStrExt;
            let c_path = match std::ffi::CString::new(path.as_os_str().as_bytes()) {
                Ok(c) => c,
                Err(_) => return,
            };

            let fd = unsafe {
                libc::open(
                    c_path.as_ptr(),
                    libc::O_PATH | libc::O_CLOEXEC | libc::O_DIRECTORY,
                )
            };
            if fd >= 0 {
                let path_attr = LandlockPathBeneathAttr {
                    allowed_access: access,
                    parent_fd: fd,
                };
                unsafe {
                    libc::syscall(
                        SYS_LANDLOCK_ADD_RULE,
                        ruleset_fd,
                        LANDLOCK_RULE_PATH_BENEATH,
                        &path_attr as *const LandlockPathBeneathAttr,
                        0u32,
                    );
                    libc::close(fd);
                }
            } else {
                let fd_file = unsafe {
                    libc::open(c_path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC)
                };
                if fd_file >= 0 {
                    let path_attr = LandlockPathBeneathAttr {
                        allowed_access: access,
                        parent_fd: fd_file,
                    };
                    unsafe {
                        libc::syscall(
                            SYS_LANDLOCK_ADD_RULE,
                            ruleset_fd,
                            LANDLOCK_RULE_PATH_BENEATH,
                            &path_attr as *const LandlockPathBeneathAttr,
                            0u32,
                        );
                        libc::close(fd_file);
                    }
                }
            }
        };

        // 3. Add read rules
        for path in &profile.allowed_reads {
            add_path_rule(path, profile.read_access & handled);
        }

        // 4. Add write rules
        for path in &profile.allowed_writes {
            add_path_rule(path, profile.write_access & handled);
        }

        // 5. Restrict process
        let prctl_res = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        if prctl_res != 0 {
            unsafe { libc::close(ruleset_fd) };
            return Err(std::io::Error::last_os_error());
        }

        let restrict_res = unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset_fd, 0u32) };
        unsafe { libc::close(ruleset_fd) };

        if restrict_res != 0 {
            return Err(std::io::Error::last_os_error());
        }

        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = profile;
        Ok(())
    }
}
