//! Command lists and the matcher (docs/11 §3).
//!
//! The pack ships four levels (`read_only` / `sensitive` / `forbidden`, plus credential
//! paths); `project` and per-scope overrides come from `command_rules`. [`Policy::classify`]
//! parses a command line with [`shell`] and returns the highest level any part of it reaches,
//! with the reasons. The matcher is not a security boundary (the sandbox is, docs/11 §5):
//! anything it cannot read statically is at least `sensitive` and marked unresolved.

mod classify;
pub mod path;
pub mod scope;
pub mod shell;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    ReadOnly,
    Project,
    Sensitive,
    Forbidden,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        Some(match s {
            "read_only" => Level::ReadOnly,
            "project" => Level::Project,
            "sensitive" => Level::Sensitive,
            "forbidden" => Level::Forbidden,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::ReadOnly => "read_only",
            Level::Project => "project",
            Level::Sensitive => "sensitive",
            Level::Forbidden => "forbidden",
        }
    }
}

/// `command_policy` as the knowledge pack ships it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PackPolicy {
    #[serde(default)]
    pub read_only: Vec<String>,
    #[serde(default)]
    pub sensitive: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
    #[serde(default)]
    pub forbidden_paths: Vec<String>,
}

impl PackPolicy {
    /// Lays a custom pack over this one. Custom packs only append to `sensitive`,
    /// `forbidden` and `forbidden_paths`; their `read_only` entries are ignored, so a pack
    /// can tighten the lists but never loosen them (docs/04 knowledge-pack §7). Loosening is
    /// the user's call, through `command_rules`.
    pub fn merged(&self, custom: Option<&PackPolicy>) -> PackPolicy {
        let mut out = self.clone();
        if let Some(c) = custom {
            for (dst, src) in [
                (&mut out.sensitive, &c.sensitive),
                (&mut out.forbidden, &c.forbidden),
                (&mut out.forbidden_paths, &c.forbidden_paths),
            ] {
                for s in src {
                    if !dst.contains(s) {
                        dst.push(s.clone());
                    }
                }
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    Digest,
    User,
    Approval,
}

impl RuleSource {
    pub fn parse(s: &str) -> Option<RuleSource> {
        Some(match s {
            "digest" => RuleSource::Digest,
            "user" => RuleSource::User,
            "approval" => RuleSource::Approval,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RuleSource::Digest => "digest",
            RuleSource::User => "user",
            RuleSource::Approval => "approval",
        }
    }
}

/// One `command_rules` row: a local override, global or for one project, for all CLIs or one.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    pub pattern: String,
    pub level: Level,
    pub source: RuleSource,
}

/// Where and for whom a command is classified. Paths are absolute; nothing here is read
/// from disk.
pub struct Context<'a> {
    pub cwd: String,
    pub home: Option<String>,
    /// Replaces `{tokage_app_data}` in `forbidden_paths`.
    pub app_data: Option<String>,
    /// Writes below these stay inside the workspace (worktree root, project root).
    pub workspace_roots: Vec<String>,
    /// The task's `write_scope`, globs relative to the first workspace root. `None` (no
    /// task, e.g. a Direct chat) means the whole workspace.
    pub write_scope: Option<Vec<String>>,
    pub default_branch: Option<String>,
    pub current_branch: Option<String>,
    pub project_id: Option<String>,
    pub provider: Option<String>,
    /// File and command names compare case-insensitively (macOS and Windows defaults).
    pub case_insensitive: bool,
    /// Whether a workspace path is tracked by git, for the "can be restored" hint.
    pub tracked: Option<&'a dyn Fn(&str) -> bool>,
}

impl<'a> Context<'a> {
    pub fn new(cwd: impl Into<String>) -> Context<'a> {
        let cwd = cwd.into();
        Context {
            workspace_roots: vec![cwd.clone()],
            cwd,
            home: None,
            app_data: None,
            write_scope: None,
            default_branch: None,
            current_branch: None,
            project_id: None,
            provider: None,
            case_insensitive: cfg!(any(target_os = "macos", windows)),
            tracked: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason {
    pub level: Level,
    /// Stable key for the UI: `rule`, `unknown_command`, `credential_path`, …
    pub code: &'static str,
    /// The command segment it applies to.
    pub segment: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub level: Level,
    pub reasons: Vec<Reason>,
    /// False when part of the command could not be read statically (a dynamic command name,
    /// a parse error, a script fed through stdin). Authorization modes treat such commands
    /// as needing a prompt even where `sensitive` would be allowed.
    pub resolved: bool,
    /// Workspace files the command writes or deletes (absolute, globs cut to their base).
    pub writes: Vec<String>,
    /// All `writes` are tracked by git; `None` without writes or without a tracker.
    pub recoverable: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
enum PosTok {
    Lit(String),
    DefaultBranch,
}

#[derive(Clone, Debug)]
struct CmdPat {
    name: String,
    flags: Vec<String>,
    pos: Vec<PosTok>,
}

impl CmdPat {
    fn specificity(&self) -> usize {
        1 + self.flags.len() + self.pos.len()
    }
}

#[derive(Clone, Debug)]
enum Pat {
    Cmd(CmdPat),
    /// `left | right` anywhere in one pipeline, left before right.
    Pipe(CmdPat, CmdPat),
    Read(String),
    Write(String),
    WriteOutside,
}

fn compile_cmd(s: &str) -> Result<CmdPat, String> {
    let mut toks = s.split_whitespace();
    let name = toks.next().ok_or("empty pattern")?;
    if name.starts_with('-') || name.contains(['<', '>', '|', ';', '&', '$', '`']) || name == "*" {
        return Err(format!("invalid command name `{name}`"));
    }
    let mut p = CmdPat {
        name: name.to_string(),
        flags: Vec::new(),
        pos: Vec::new(),
    };
    for t in toks {
        match t {
            "*" => {}
            "<default-branch>" => p.pos.push(PosTok::DefaultBranch),
            t if t.starts_with('<') => return Err(format!("unknown placeholder `{t}`")),
            t if t.starts_with('-') && t.len() > 1 => p.flags.push(t.to_string()),
            t => {
                if p.pos.contains(&PosTok::DefaultBranch) {
                    return Err("`<default-branch>` must come last".into());
                }
                p.pos.push(PosTok::Lit(t.to_string()))
            }
        }
    }
    Ok(p)
}

fn compile(s: &str) -> Result<Pat, String> {
    let s = s.trim();
    if let Some(g) = s.strip_prefix("read:") {
        return if g.is_empty() { Err("empty path".into()) } else { Ok(Pat::Read(g.to_string())) };
    }
    if let Some(g) = s.strip_prefix("write:") {
        return match g {
            "<outside-workspace>" => Ok(Pat::WriteOutside),
            "" => Err("empty path".into()),
            g if g.contains('<') => Err(format!("unknown placeholder `{g}`")),
            g => Ok(Pat::Write(g.to_string())),
        };
    }
    if let Some((l, r)) = s.split_once(" | ") {
        return Ok(Pat::Pipe(compile_cmd(l)?, compile_cmd(r)?));
    }
    if s.replace("<default-branch>", "").contains(['|', ';', '&', '`', '$', '<', '>']) {
        return Err("patterns are one command (`a | b` for pipelines)".into());
    }
    compile_cmd(s).map(Pat::Cmd)
}

#[derive(Clone, Debug)]
struct Entry {
    pat: Pat,
    level: Level,
    text: String,
    /// From `command_rules` rather than the pack; wins ties on specificity.
    user: bool,
    project_id: Option<String>,
    provider: Option<String>,
}

impl Entry {
    fn applies(&self, ctx: &Context) -> bool {
        self.project_id.as_ref().map_or(true, |p| Some(p) == ctx.project_id.as_ref())
            && self.provider.as_ref().map_or(true, |p| Some(p) == ctx.provider.as_ref())
    }
}

/// The compiled lists: pack (built-in plus custom) and local rules.
#[derive(Clone, Debug)]
pub struct Policy {
    entries: Vec<Entry>,
    forbidden_paths: Vec<String>,
    /// Left-hand commands of forbidden pipe patterns (`curl * | sh`): their output must
    /// not become a script however it is fed in.
    fetchers: Vec<String>,
}

impl Policy {
    /// Invalid pack patterns are skipped; invalid rules too (they are checked when added).
    pub fn new(pack: &PackPolicy, rules: &[Rule]) -> Policy {
        let mut entries = Vec::new();
        for (list, level) in [
            (&pack.read_only, Level::ReadOnly),
            (&pack.sensitive, Level::Sensitive),
            (&pack.forbidden, Level::Forbidden),
        ] {
            for text in list {
                if let Ok(pat) = compile(text) {
                    entries.push(Entry {
                        pat,
                        level,
                        text: text.clone(),
                        user: false,
                        project_id: None,
                        provider: None,
                    });
                }
            }
        }
        for r in rules {
            if let Ok(pat) = compile(&r.pattern) {
                entries.push(Entry {
                    pat,
                    level: r.level,
                    text: r.pattern.clone(),
                    user: true,
                    project_id: r.project_id.clone(),
                    provider: r.provider.clone(),
                });
            }
        }
        let mut fetchers = Vec::new();
        for e in &entries {
            if let (Pat::Pipe(l, _), Level::Forbidden) = (&e.pat, e.level) {
                if !fetchers.contains(&l.name) {
                    fetchers.push(l.name.clone());
                }
            }
        }
        Policy {
            entries,
            forbidden_paths: pack.forbidden_paths.clone(),
            fetchers,
        }
    }

    pub fn classify(&self, command: &str, ctx: &Context) -> Verdict {
        classify::run(self, command, ctx)
    }

    /// Classifies a file edit made through the CLI's edit tool. `file` is absolute or
    /// relative to `ctx.cwd`. Inside the workspace and `write_scope` it is `project`; path
    /// rules (`write:.env*`), `.git` and leaving `write_scope` make it `sensitive`; outside
    /// the workspace or on a credential path it is `forbidden`.
    pub fn classify_edit(&self, file: &str, ctx: &Context) -> Verdict {
        classify::edit(self, file, ctx)
    }

    /// Classifies a file read made through the CLI's read tool. `file` is absolute or
    /// relative to `ctx.cwd`. Reading a credential path or matching `forbidden_paths` is `forbidden`;
    /// rules (`read:<glob>`) and defaults apply.
    pub fn classify_read(&self, file: &str, ctx: &Context) -> Verdict {
        classify::read(self, file, ctx)
    }

    /// Checks a rule before it is stored. A rule below `forbidden` may not cover anything
    /// the pack forbids: `forbidden` can only be added to, never removed (docs/11 §3.3).
    pub fn validate_rule(&self, rule: &Rule) -> Result<(), String> {
        let pat = compile(&rule.pattern)?;
        if rule.level == Level::Forbidden {
            return Ok(());
        }
        let shadowed = match &pat {
            Pat::Cmd(c) => self.entries.iter().any(|e| {
                !e.user
                    && e.level == Level::Forbidden
                    && matches!(&e.pat, Pat::Cmd(f) if pattern_covers(f, c))
            }),
            Pat::Pipe(..) | Pat::WriteOutside => true,
            Pat::Read(g) | Pat::Write(g) => {
                let g = g.trim_start_matches("./");
                self.forbidden_paths.iter().any(|f| {
                    let fb = path::literal_base(f);
                    let gb = path::literal_base(g);
                    !gb.is_empty() && (path::under(&gb, &fb) || path::under(&fb, &gb))
                })
            }
        };
        if shadowed {
            Err(format!("`{}` is forbidden and cannot be allowed", rule.pattern))
        } else {
            Ok(())
        }
    }
}

/// The forbidden pattern `f` matches every command the pattern `c` matches.
fn pattern_covers(f: &CmdPat, c: &CmdPat) -> bool {
    f.name.eq_ignore_ascii_case(&c.name)
        && f.flags.iter().all(|x| c.flags.contains(x))
        && f.pos.iter().zip(c.pos.iter().map(Some).chain(std::iter::repeat(None))).all(|(a, b)| match (a, b) {
            (PosTok::DefaultBranch, _) => true,
            (a, Some(b)) => a == b,
            (_, None) => false,
        })
}
