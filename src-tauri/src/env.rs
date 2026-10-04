//! Login-shell environment capture (docs/01 §3). Values stay in memory only.

use std::{collections::HashMap, path::PathBuf, time::Duration};

#[derive(Clone, Debug, Default)]
pub struct ShellEnv {
    pub vars: HashMap<String, String>,
}

impl ShellEnv {
    pub fn path(&self) -> Option<&str> {
        self.vars.get("PATH").map(String::as_str)
    }
}

/// User-level install locations that GUI-launched apps often miss even when the login
/// shell capture works (e.g. rc files that only set PATH for interactive shells).
/// Appended after the captured PATH, so the user's own ordering always wins.
#[cfg(unix)]
const EXTRA_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "~/.local/bin",      // uv tools (aider), pipx, Claude Code native installer
    "~/.npm-global/bin", // common npm prefix
    "~/.claude/local",   // legacy Claude Code local install
    "~/.gemini/bin",     // Antigravity CLI (agy) self-update location
    "~/.cargo/bin",
    "~/.bun/bin",
    "~/.volta/bin",
    "~/.deno/bin",
    "~/Library/pnpm",
    "~/.local/share/pnpm",
    "~/Library/Python/*/bin", // pip install --user on macOS
    "~/.nvm/versions/node/*/bin",
];

#[cfg(windows)]
const EXTRA_DIRS: &[&str] = &[
    "%APPDATA%\\npm",
    "%USERPROFILE%\\.local\\bin",
    "%LOCALAPPDATA%\\agy\\bin",
    "%APPDATA%\\Python\\*\\Scripts",
    "%LOCALAPPDATA%\\Programs\\Python\\*\\Scripts",
];

pub async fn capture() -> ShellEnv {
    let mut vars = match capture_login_shell().await {
        Some(vars) => vars,
        None => std::env::vars().collect(),
    };
    augment_path(&mut vars);
    ShellEnv { vars }
}

/// Variables passed into the clean login shell: identity, locale and agent sockets only.
#[cfg(unix)]
const INHERITED: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SSH_AUTH_SOCK",
    "__CF_USER_TEXT_ENCODING",
];

#[cfg(unix)]
async fn capture_login_shell() -> Option<HashMap<String, String>> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    // Start from a clean environment so the result is what the user's own shell config
    // produces, not whatever the launching process had. Editors (VS Code, Cursor) inject
    // extension shims into their integrated terminals' PATH, e.g. Copilot Chat's bundled
    // `copilot`, which is not a real installation.
    let child = tokio::process::Command::new(shell)
        .args(["-ilc", "env -0"])
        .env_clear()
        .envs(
            INHERITED
                .iter()
                .filter_map(|k| std::env::var(k).ok().map(|v| (*k, v))),
        )
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(5), child)
        .await
        .ok()?
        .ok()?;
    if !out.status.success() {
        tracing::warn!("login shell env capture exited with {}", out.status);
        return None;
    }
    let vars: HashMap<_, _> = out
        .stdout
        .split(|b| *b == 0)
        .filter_map(|entry| {
            let s = std::str::from_utf8(entry).ok()?;
            let (k, v) = s.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect();
    vars.contains_key("PATH").then_some(vars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_shims_are_dropped_from_path() {
        let mut vars = HashMap::new();
        let shim = "/Users/me/Library/Application Support/Code/User/globalStorage/github.copilot-chat/copilotCli";
        vars.insert("PATH".to_string(), format!("/usr/bin:{shim}:/bin"));
        vars.insert("HOME".to_string(), "/nonexistent-home".to_string());
        augment_path(&mut vars);
        let path = &vars["PATH"];
        assert!(!path.contains("globalStorage"), "{path}");
        assert!(path.starts_with("/usr/bin:/bin"), "{path}");
        assert!(is_editor_shim_dir(
            "C:\\Users\\me\\AppData\\Roaming\\Code\\User\\globalStorage\\x"
        ));
        assert!(!is_editor_shim_dir("/Users/me/.local/bin"));
    }
}

#[cfg(not(unix))]
async fn capture_login_shell() -> Option<HashMap<String, String>> {
    None
}

fn augment_path(vars: &mut HashMap<String, String>) {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut entries: Vec<String> = vars
        .get("PATH")
        .map(|p| {
            p.split(sep)
                .filter(|e| !e.is_empty() && !is_editor_shim_dir(e))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for pattern in EXTRA_DIRS {
        for dir in expand(pattern, vars) {
            let dir = dir.to_string_lossy().into_owned();
            if !entries.contains(&dir) {
                entries.push(dir);
            }
        }
    }
    vars.insert("PATH".into(), entries.join(&sep.to_string()));
}

/// PATH entries that point into an editor's extension storage. These hold shims bundled
/// with extensions (Copilot Chat ships a `copilot` wrapper there), not CLI installs.
/// They also get in via the fallback path (e.g. rc files that only run inside the editor).
fn is_editor_shim_dir(dir: &str) -> bool {
    let d = dir.replace('\\', "/").to_ascii_lowercase();
    d.contains("/user/globalstorage/")
        || d.contains("/.vscode/extensions/")
        || d.contains("/.vscode-server/")
        || d.contains("/.cursor/extensions/")
        || d.contains("/.cursor-server/")
        || d.contains("/.windsurf/extensions/")
}

/// Expands `~` / `%VAR%` and a single `*` path segment; returns only existing directories.
fn expand(pattern: &str, vars: &HashMap<String, String>) -> Vec<PathBuf> {
    let get = |k: &str| vars.get(k).cloned().or_else(|| std::env::var(k).ok());
    let mut s = pattern.to_string();
    if let Some(rest) = s.strip_prefix('~') {
        let Some(home) = get("HOME").or_else(|| get("USERPROFILE")) else {
            return vec![];
        };
        s = format!("{home}{rest}");
    }
    while let Some(start) = s.find('%') {
        let Some(len) = s[start + 1..].find('%') else {
            break;
        };
        let key = &s[start + 1..start + 1 + len];
        let Some(val) = get(key) else { return vec![] };
        s.replace_range(start..start + len + 2, &val);
    }

    let path = PathBuf::from(&s);
    let parts: Vec<_> = path.components().collect();
    let Some(star) = parts.iter().position(|c| c.as_os_str() == "*") else {
        return if path.is_dir() { vec![path] } else { vec![] };
    };
    let base: PathBuf = parts[..star].iter().collect();
    let tail: PathBuf = parts[star + 1..].iter().collect();
    let Ok(read) = std::fs::read_dir(&base) else {
        return vec![];
    };
    let mut found: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path().join(&tail))
        .filter(|p| p.is_dir())
        .collect();
    // Newest versions first (Python/3.12 before 3.9, node v22 before v18).
    found.sort();
    found.reverse();
    found
}
