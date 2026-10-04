//! File changes made during a turn, turned into unified diffs.
//!
//! CLIs report edits in different shapes (full before/after, a patch, or only a path), and
//! some edits happen through shell commands with no edit event at all. So the previous
//! content comes from, in order: what the CLI reported, what this turn already saw, or git
//! HEAD for files that were clean when the turn started. At the end of the turn, `git status`
//! catches changes no event mentioned.
//!
//! Files outside the working directory (a CLI's plan in its home folder, say) can't be read
//! back by the preview pane, which stays inside the project. Their content is copied into
//! Tokage's own folder, `<app data>/files/<project folder>/<date>/<name>`, and the event
//! points there.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use similar::TextDiff;

use super::{Change, EventKind, FileTouch};
use crate::{env::ShellEnv, git};

const DIFF_MAX: usize = 1024 * 1024;

pub struct Files {
    cwd: PathBuf,
    /// Repository root, when the working directory is inside one.
    root: Option<PathBuf>,
    /// Paths (relative to `root`) that were already dirty or untracked at turn start;
    /// directory entries end with `/`.
    dirty: HashSet<String>,
    /// Content before this turn first touched each path (`None` inside = didn't exist;
    /// outer `None` = unknown). Diffs are cumulative from here, so the last event per path
    /// shows the turn's whole change to that file.
    first: HashMap<PathBuf, Option<Option<String>>>,
    /// Where copies of outside files go (`<app data>/files`); `None` disables copying.
    mirror: Option<PathBuf>,
    /// Copy path per outside file, so later edits in the turn overwrite the same copy.
    copies: HashMap<PathBuf, PathBuf>,
}

impl Files {
    pub async fn start(env: &ShellEnv, cwd: &str, mirror: Option<PathBuf>) -> Self {
        let cwd = PathBuf::from(cwd);
        let cwd = cwd.canonicalize().unwrap_or(cwd);
        let mut f = Self {
            root: None,
            dirty: HashSet::new(),
            first: HashMap::new(),
            mirror,
            copies: HashMap::new(),
            cwd,
        };
        if let Ok(top) = git::run(env, &f.cwd, &["rev-parse", "--show-toplevel"], &[], 10).await {
            let top = PathBuf::from(top.trim());
            f.root = Some(top.canonicalize().unwrap_or(top));
            f.dirty = status_paths(env, &f.cwd).await.into_iter().collect();
        }
        f
    }

    fn abs(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        let p = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        };
        // The file may be gone; canonicalise the parent instead.
        match (p.parent().and_then(|d| d.canonicalize().ok()), p.file_name()) {
            (Some(d), Some(n)) => d.join(n),
            _ => p,
        }
    }

    fn display(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.cwd)
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_else(|_| abs.to_string_lossy().into_owned())
    }

    fn was_dirty(&self, rel: &str) -> bool {
        self.dirty.contains(rel)
            || self
                .dirty
                .iter()
                .any(|d| d.ends_with('/') && rel.starts_with(d.as_str()))
    }

    /// Content before the turn touched `abs`; outer `None` when it can't be known.
    async fn baseline(&self, env: &ShellEnv, abs: &Path) -> Option<Option<String>> {
        let root = self.root.as_ref()?;
        let rel = abs.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
        if self.was_dirty(&rel) {
            return None;
        }
        let spec = format!("HEAD:{rel}");
        match git::run(env, root, &["show", &spec], &[], 10).await {
            Ok(text) => Some(Some(text)),
            // Clean and not in HEAD: the file didn't exist.
            Err(_) => Some(None),
        }
    }

    pub async fn change(&mut self, env: &ShellEnv, t: FileTouch) -> EventKind {
        let abs = self.abs(&t.path);
        let before = match self.first.get(&abs) {
            Some(b) => b.clone(),
            None => {
                let b = match t.before {
                    Some(b) => Some(b),
                    None => self.baseline(env, &abs).await,
                };
                self.first.insert(abs.clone(), b.clone());
                b
            }
        };
        let after = if t.deleted {
            None
        } else {
            t.after.or_else(|| read(&abs))
        };
        self.event(t.tool_id, &abs, before, after)
    }

    /// Copies an outside file's current content into the mirror folder; returns the copy.
    fn copy_out(&mut self, abs: &Path, after: Option<&str>) -> Option<String> {
        let (Some(base), Some(text)) = (&self.mirror, after) else {
            return None;
        };
        if abs.starts_with(&self.cwd) {
            return None;
        }
        let dest = match self.copies.get(abs) {
            Some(d) => d.clone(),
            None => {
                let dir = base.join(safe_name(&self.cwd)).join(today());
                let name = abs.file_name()?.to_string_lossy().into_owned();
                // Two outside files with the same name in one turn get separate copies.
                let mut dest = dir.join(&name);
                let mut n = 2;
                while self.copies.values().any(|d| d == &dest) {
                    dest = dir.join(numbered(&name, n));
                    n += 1;
                }
                self.copies.insert(abs.to_path_buf(), dest.clone());
                dest
            }
        };
        std::fs::create_dir_all(dest.parent()?).ok()?;
        std::fs::write(&dest, text).ok()?;
        Some(dest.to_string_lossy().into_owned())
    }

    fn event(
        &mut self,
        tool_id: Option<String>,
        abs: &Path,
        before: Option<Option<String>>,
        after: Option<String>,
    ) -> EventKind {
        let path = self.display(abs);
        let change = match (&before, &after) {
            (Some(None), _) => Change::Added,
            (_, None) => Change::Deleted,
            _ => Change::Modified,
        };
        let (diff, additions, deletions) = match before {
            Some(b) => unified(&path, b.as_deref(), after.as_deref()),
            // Unknown baseline: a new file still shows its content as additions.
            None => (None, 0, 0),
        };
        let preview_path = self.copy_out(abs, after.as_deref());
        EventKind::FileChange {
            tool_id,
            path,
            abs_path: abs.to_string_lossy().into_owned(),
            preview_path,
            change,
            diff,
            additions,
            deletions,
        }
    }

    /// Changes made outside any edit event (shell redirects, scripts, formatters).
    pub async fn finish(&mut self, env: &ShellEnv) -> Vec<EventKind> {
        let Some(root) = self.root.clone() else {
            return vec![];
        };
        let mut evs = vec![];
        for rel in status_paths(env, &self.cwd).await {
            if rel.ends_with('/') || self.was_dirty(&rel) {
                continue;
            }
            let abs = root.join(&rel);
            if self.first.contains_key(&abs) || !abs.starts_with(&self.cwd) {
                continue;
            }
            let before = self.baseline(env, &abs).await;
            let after = read(&abs);
            if before.as_ref() == Some(&after) {
                continue;
            }
            self.first.insert(abs.clone(), before.clone());
            evs.push(self.event(None, &abs, before, after));
        }
        evs
    }
}

/// Last path component of `dir`, limited to characters safe in a folder name.
fn safe_name(dir: &Path) -> String {
    let n: String = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' })
        .collect();
    let n = n.trim_start_matches('.').to_string();
    if n.is_empty() { "project".into() } else { n }
}

/// `plan.md` → `plan-2.md`.
fn numbered(name: &str, n: u32) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{n}.{ext}"),
        _ => format!("{name}-{n}"),
    }
}

/// Local date as `YYYY-MM-DD`.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil(secs + utc_offset(secs));
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(unix)]
fn utc_offset(secs: i64) -> i64 {
    let t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call; localtime_r is the thread-safe variant.
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i64
}

#[cfg(not(unix))]
fn utc_offset(_secs: i64) -> i64 {
    0
}

/// Days-from-epoch to civil date (Howard Hinnant's algorithm).
fn civil(secs: i64) -> (i64, u32, u32) {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn read(p: &Path) -> Option<String> {
    let bytes = std::fs::read(p).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// `git status` paths relative to the repository root.
async fn status_paths(env: &ShellEnv, cwd: &Path) -> Vec<String> {
    let Ok(out) = git::run(
        env,
        cwd,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        &[],
        15,
    )
    .await
    else {
        return vec![];
    };
    let mut paths = vec![];
    let mut it = out.split('\0');
    while let Some(entry) = it.next() {
        if entry.len() < 4 {
            continue;
        }
        let (xy, path) = entry.split_at(3);
        paths.push(path.to_string());
        // Renames carry the original path as the next entry.
        if xy.starts_with('R') || xy.starts_with('C') {
            it.next();
        }
    }
    paths
}

/// Unified diff with line counts; `None` diff when binary or too large.
pub fn unified(path: &str, before: Option<&str>, after: Option<&str>) -> (Option<String>, u32, u32) {
    let (a, b) = (before.unwrap_or(""), after.unwrap_or(""));
    if a.len() + b.len() > DIFF_MAX || a.contains('\0') || b.contains('\0') {
        return (None, 0, 0);
    }
    let diff = TextDiff::from_lines(a, b);
    let (mut add, mut del) = (0, 0);
    for op in diff.ops() {
        for c in diff.iter_changes(op) {
            match c.tag() {
                similar::ChangeTag::Insert => add += 1,
                similar::ChangeTag::Delete => del += 1,
                similar::ChangeTag::Equal => {}
            }
        }
    }
    let old = if before.is_some() { format!("a/{path}") } else { "/dev/null".into() };
    let new = if after.is_some() { format!("b/{path}") } else { "/dev/null".into() };
    let text = diff
        .unified_diff()
        .context_radius(3)
        .header(&old, &new)
        .to_string();
    (Some(text), add, del)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_counts() {
        let (d, a, r) = unified("m.py", Some("x\ny\n"), Some("x\nz\n"));
        let d = d.unwrap();
        assert!(d.starts_with("--- a/m.py\n+++ b/m.py\n"), "{d}");
        assert!(d.contains("-y\n+z\n"));
        assert_eq!((a, r), (1, 1));
        let (d, a, _) = unified("n.txt", None, Some("hi\n"));
        assert!(d.unwrap().starts_with("--- /dev/null"));
        assert_eq!(a, 1);
    }

    #[tokio::test]
    async fn baseline_from_git_and_shell_edits() {
        let env = ShellEnv {
            vars: std::env::vars().collect(),
        };
        let dir = std::env::temp_dir().join(format!("tokage-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .unwrap();
        };
        g(&["init", "-q"]);
        std::fs::write(dir.join("m.py"), "a-b\n").unwrap();
        std::fs::write(dir.join("dirty.txt"), "1\n").unwrap();
        g(&["add", "."]);
        g(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "i"]);
        std::fs::write(dir.join("dirty.txt"), "2\n").unwrap();

        let mut f = Files::start(&env, &dir.to_string_lossy(), Some(dir.join("mirror"))).await;
        // The CLI only reports the path; the baseline comes from HEAD.
        std::fs::write(dir.join("m.py"), "a+b\n").unwrap();
        let ev = f
            .change(
                &env,
                FileTouch {
                    tool_id: None,
                    path: "m.py".into(),
                    before: None,
                    after: None,
                    deleted: false,
                },
            )
            .await;
        match ev {
            EventKind::FileChange {
                change,
                additions,
                deletions,
                diff,
                path,
                ..
            } => {
                assert_eq!(path, "m.py");
                assert_eq!(change, Change::Modified);
                assert_eq!((additions, deletions), (1, 1));
                assert!(diff.unwrap().contains("+a+b"));
            }
            e => panic!("{e:?}"),
        }
        // A second edit diffs against the turn's starting content.
        std::fs::write(dir.join("m.py"), "a*b\n").unwrap();
        let ev = f
            .change(&env, FileTouch { tool_id: None, path: "m.py".into(), before: Some(Some("a+b\n".into())), after: None, deleted: false })
            .await;
        assert!(matches!(ev, EventKind::FileChange { ref diff, .. } if diff.as_deref().unwrap().contains("-a-b\n+a*b")), "{ev:?}");
        // A shell redirect creates a file; dirty-at-start files are left alone.
        std::fs::write(dir.join("hello.txt"), "hi\n").unwrap();
        std::fs::write(dir.join("dirty.txt"), "3\n").unwrap();
        let rest = f.finish(&env).await;
        assert_eq!(rest.len(), 1, "{rest:?}");
        assert!(matches!(&rest[0], EventKind::FileChange { path, change: Change::Added, .. } if path == "hello.txt"));

        // A file outside the working directory is copied where the preview can read it.
        let outside = std::env::temp_dir().join(format!("tokage-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("plan.md"), "# Plan\n").unwrap();
        let ev = f
            .change(
                &env,
                FileTouch {
                    tool_id: None,
                    path: outside.join("plan.md").to_string_lossy().into_owned(),
                    before: Some(None),
                    after: None,
                    deleted: false,
                },
            )
            .await;
        let EventKind::FileChange { preview_path: Some(copy), .. } = ev else { panic!("{ev:?}") };
        assert!(copy.starts_with(&dir.join("mirror").to_string_lossy().into_owned()), "{copy}");
        assert!(copy.ends_with(&format!("/{}/plan.md", today())), "{copy}");
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "# Plan\n");
        // Files inside the working directory are read in place.
        assert!(matches!(&rest[0], EventKind::FileChange { preview_path: None, .. }));
        std::fs::remove_dir_all(&outside).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dates_and_names() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(1_791_072_000), (2026, 10, 4));
        assert_eq!(numbered("plan.md", 2), "plan-2.md");
        assert_eq!(numbered("Makefile", 3), "Makefile-3");
        assert_eq!(safe_name(Path::new("/a/my proj")), "my_proj");
        assert_eq!(safe_name(Path::new("/a/..")), "project");
    }
}
