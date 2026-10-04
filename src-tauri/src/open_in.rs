//! "Open project in…" actions: file manager, VS Code, terminal.
//! Targets are a fixed enum and the path is passed as a single argument, never through a shell.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::ShellEnv;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OpenTarget {
    FileManager,
    VsCode,
    Terminal,
}

#[cfg(target_os = "macos")]
const VSCODE_APP: &str = "/Applications/Visual Studio Code.app";

fn vscode_cli(env: &ShellEnv) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    which::which_in("code", env.path(), cwd).ok()
}

fn has_vscode(env: &ShellEnv) -> bool {
    #[cfg(target_os = "macos")]
    if Path::new(VSCODE_APP).is_dir() {
        return true;
    }
    vscode_cli(env).is_some()
}

pub fn available(env: &ShellEnv) -> Vec<OpenTarget> {
    let mut list = vec![OpenTarget::FileManager];
    if has_vscode(env) {
        list.push(OpenTarget::VsCode);
    }
    list.push(OpenTarget::Terminal);
    list
}

pub async fn open(env: &ShellEnv, path: &Path, target: OpenTarget) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("not a directory: {}", path.display()));
    }
    let mut cmd = command(env, path, target)?;
    cmd.env_clear()
        .envs(&env.vars)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Launchers return immediately; the opened app keeps running on its own.
    cmd.spawn().map(drop).map_err(|e| e.to_string())
}

/// Opens an http(s) link in the default browser. Other schemes are refused.
pub async fn open_url(env: &ShellEnv, url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) || url.chars().any(char::is_whitespace) {
        return Err("Only http(s) links can be opened".into());
    }
    #[cfg(target_os = "macos")]
    let mut cmd = tokio::process::Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut cmd = tokio::process::Command::new("explorer.exe");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = tokio::process::Command::new("xdg-open");
    cmd.arg(url)
        .env_clear()
        .envs(&env.vars)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn().map(drop).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn command(
    env: &ShellEnv,
    path: &Path,
    target: OpenTarget,
) -> Result<tokio::process::Command, String> {
    let mut cmd = tokio::process::Command::new("/usr/bin/open");
    match target {
        OpenTarget::FileManager => {}
        OpenTarget::Terminal => {
            cmd.args(["-a", "Terminal"]);
        }
        OpenTarget::VsCode if Path::new(VSCODE_APP).is_dir() => {
            cmd.arg("-a").arg(VSCODE_APP);
        }
        OpenTarget::VsCode => {
            let code = vscode_cli(env).ok_or("VS Code not found")?;
            let mut c = tokio::process::Command::new(code);
            c.arg(path);
            return Ok(c);
        }
    }
    cmd.arg(path);
    Ok(cmd)
}

#[cfg(target_os = "windows")]
fn command(
    env: &ShellEnv,
    path: &Path,
    target: OpenTarget,
) -> Result<tokio::process::Command, String> {
    Ok(match target {
        OpenTarget::FileManager => {
            let mut c = tokio::process::Command::new("explorer.exe");
            c.arg(path);
            c
        }
        OpenTarget::VsCode => {
            let mut c = tokio::process::Command::new(vscode_cli(env).ok_or("VS Code not found")?);
            c.arg(path);
            c
        }
        OpenTarget::Terminal => {
            let mut c = tokio::process::Command::new("wt.exe");
            c.arg("-d").arg(path);
            c
        }
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn command(
    env: &ShellEnv,
    path: &Path,
    target: OpenTarget,
) -> Result<tokio::process::Command, String> {
    Ok(match target {
        OpenTarget::FileManager => {
            let mut c = tokio::process::Command::new("xdg-open");
            c.arg(path);
            c
        }
        OpenTarget::VsCode => {
            let mut c = tokio::process::Command::new(vscode_cli(env).ok_or("VS Code not found")?);
            c.arg(path);
            c
        }
        OpenTarget::Terminal => {
            let mut c = tokio::process::Command::new("x-terminal-emulator");
            c.current_dir(path);
            c
        }
    })
}
