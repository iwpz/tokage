//! Project context directory `.tokage/` (docs/08 §2.3, docs/02 §6).
//!
//! Tokage writes plans into the project instead of relying on each CLI's private folder, and
//! points every CLI at them through `AGENTS.md` (Claude Code reads it via `@AGENTS.md` in
//! `CLAUDE.md`). Layout:
//!
//! ```text
//! .tokage/
//!   context.md                     constraints and task state, shared by all sessions
//!   plans/<session-id>.md          latest plan of one session
//!   plans/history/<session-id>/N.md  earlier versions, N = 1, 2, ...
//! ```
//!
//! Outside `.tokage/` Tokage only touches `AGENTS.md` (its marked block), `CLAUDE.md` (one
//! `@AGENTS.md` line) and, when the user agrees, `.gitignore` (one `.tokage/` line).

use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

pub const DIR: &str = ".tokage";
const BEGIN: &str = "<!-- tokage:begin -->";
const END: &str = "<!-- tokage:end -->";
const CLAUDE_REF: &str = "@AGENTS.md";
const IGNORE_LINE: &str = ".tokage/";

const AGENTS_BLOCK: &str = "## Tokage project context

This project is worked on through Tokage. Shared context lives in `.tokage/`:

- `.tokage/context.md`: constraints and task state. Read it before starting work.
- `.tokage/plans/<session-id>.md`: the current plan of each Tokage session. When a prompt names a plan file, follow it.

Do not edit files under `.tokage/` unless asked; Tokage maintains them.";

const CONTEXT_SKELETON: &str = "# Project context

Maintained by Tokage. Constraints and task state shared by all sessions.

## Constraints

## Task state
";

/// Relative path of a session's plan file.
pub fn plan_rel(session_id: &str) -> Result<String, String> {
    let ok = !session_id.is_empty()
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(format!("Invalid session id: {session_id}"));
    }
    Ok(format!("{DIR}/plans/{session_id}.md"))
}

/// Whether `.tokage/` exists yet (the first write asks about `.gitignore`).
pub fn exists(root: &Path) -> bool {
    root.join(DIR).is_dir()
}

#[derive(Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedPlan {
    pub path: String,
    /// 1 for the first version.
    pub version: u32,
}

/// Writes a session's plan, keeping the previous one under `plans/history/`. Also makes sure
/// `context.md`, the `AGENTS.md` block and the `CLAUDE.md` reference exist.
pub fn save_plan(root: &Path, session_id: &str, title: &str, text: &str) -> Result<SavedPlan, String> {
    let rel = plan_rel(session_id)?;
    let file = root.join(&rel);
    let history = root.join(DIR).join("plans/history").join(session_id);
    let res: io::Result<u32> = (|| {
        fs::create_dir_all(file.parent().unwrap())?;
        let mut version = 1;
        if let Ok(prev) = fs::read_to_string(&file) {
            fs::create_dir_all(&history)?;
            let n = count_versions(&history) + 1;
            fs::write(history.join(format!("{n}.md")), prev)?;
            version = n + 1;
        }
        let title = title.trim().replace('\n', " ");
        let body = format!(
            "<!-- Tokage plan · session {session_id} · version {version} -->\n# {}\n\n{}\n",
            if title.is_empty() { "Plan" } else { &title },
            text.trim()
        );
        fs::write(&file, body)?;
        Ok(version)
    })();
    let version = res.map_err(|e| format!("Failed to write {rel}: {e}"))?;
    ensure_context(root).map_err(|e| format!("Failed to update project context: {e}"))?;
    Ok(SavedPlan { path: rel, version })
}

fn count_versions(dir: &Path) -> u32 {
    fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str()?.strip_suffix(".md")?.parse::<u32>().ok())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

/// `context.md`, the `AGENTS.md` block and the `CLAUDE.md` reference. Idempotent.
pub fn ensure_context(root: &Path) -> io::Result<()> {
    let ctx = root.join(DIR).join("context.md");
    if !ctx.exists() {
        fs::create_dir_all(ctx.parent().unwrap())?;
        fs::write(&ctx, CONTEXT_SKELETON)?;
    }
    write_if_changed(&root.join("AGENTS.md"), |old| with_block(old.unwrap_or("")))?;
    write_if_changed(&root.join("CLAUDE.md"), |old| with_line(old, CLAUDE_REF))?;
    Ok(())
}

/// Adds `.tokage/` to the project's `.gitignore` (the user agreed). Idempotent.
pub fn add_to_gitignore(root: &Path) -> io::Result<()> {
    write_if_changed(&root.join(".gitignore"), |old| with_line(old, IGNORE_LINE))
}

fn write_if_changed(path: &PathBuf, f: impl FnOnce(Option<&str>) -> String) -> io::Result<()> {
    let old = match fs::read_to_string(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let new = f(old.as_deref());
    if old.as_deref() != Some(new.as_str()) {
        fs::write(path, new)?;
    }
    Ok(())
}

/// Replaces the marked Tokage block, or appends one.
fn with_block(old: &str) -> String {
    let block = format!("{BEGIN}\n{AGENTS_BLOCK}\n{END}\n");
    if let (Some(b), Some(e)) = (old.find(BEGIN), old.find(END)) {
        if b < e {
            let tail = old[e + END.len()..].strip_prefix('\n').unwrap_or(&old[e + END.len()..]);
            return format!("{}{block}{tail}", &old[..b]);
        }
    }
    if old.trim().is_empty() {
        return format!("# AGENTS.md\n\n{block}");
    }
    format!("{}\n\n{block}", old.trim_end())
}

/// Appends `line` unless some line already equals it.
fn with_line(old: Option<&str>, line: &str) -> String {
    let old = old.unwrap_or("");
    if old.lines().any(|l| l.trim() == line) {
        return old.to_string();
    }
    if old.is_empty() {
        return format!("{line}\n");
    }
    let sep = if old.ends_with('\n') { "" } else { "\n" };
    format!("{old}{sep}{line}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_versions() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        assert!(!exists(r));
        let a = save_plan(r, "s-1", "Login", "v1").unwrap();
        assert_eq!(a, SavedPlan { path: ".tokage/plans/s-1.md".into(), version: 1 });
        let b = save_plan(r, "s-1", "Login", "v2").unwrap();
        assert_eq!(b.version, 2);
        let cur = fs::read_to_string(r.join(&b.path)).unwrap();
        assert!(cur.contains("# Login") && cur.contains("v2"));
        let old = fs::read_to_string(r.join(".tokage/plans/history/s-1/1.md")).unwrap();
        assert!(old.contains("v1"));
        assert_eq!(save_plan(r, "s-1", "", "v3").unwrap().version, 3);
        assert!(r.join(".tokage/context.md").exists());
        assert!(save_plan(r, "../x", "", "").is_err());
    }

    #[test]
    fn agents_and_claude_files() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        fs::write(r.join("AGENTS.md"), "# Rules\n\nUse tabs.\n").unwrap();
        fs::write(r.join("CLAUDE.md"), "Be brief.").unwrap();
        ensure_context(r).unwrap();
        ensure_context(r).unwrap();
        let agents = fs::read_to_string(r.join("AGENTS.md")).unwrap();
        assert!(agents.starts_with("# Rules\n\nUse tabs.\n\n<!-- tokage:begin -->"));
        assert_eq!(agents.matches(BEGIN).count(), 1);
        assert_eq!(fs::read_to_string(r.join("CLAUDE.md")).unwrap(), "Be brief.\n@AGENTS.md\n");
        // A stale block is replaced in place; text after it stays.
        fs::write(r.join("AGENTS.md"), format!("A\n{BEGIN}\nold\n{END}\nB\n")).unwrap();
        ensure_context(r).unwrap();
        let agents = fs::read_to_string(r.join("AGENTS.md")).unwrap();
        assert!(!agents.contains("old") && agents.starts_with("A\n") && agents.ends_with("{END}\nB\n".replace("{END}", END).as_str()));
    }

    #[test]
    fn gitignore_line() {
        let d = tempfile::tempdir().unwrap();
        add_to_gitignore(d.path()).unwrap();
        fs::write(d.path().join(".gitignore"), "node_modules").unwrap();
        add_to_gitignore(d.path()).unwrap();
        add_to_gitignore(d.path()).unwrap();
        assert_eq!(fs::read_to_string(d.path().join(".gitignore")).unwrap(), "node_modules\n.tokage/\n");
    }
}
