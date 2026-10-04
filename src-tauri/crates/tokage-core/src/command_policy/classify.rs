//! Classification of a parsed command line (docs/11 §3.2). Every segment, substitution and
//! nested command (`bash -c`, `find -exec`, `xargs`, `env`, …) is classified on its own; the
//! verdict is the highest level reached. Paths are checked lexically against
//! `forbidden_paths`, `read:` / `write:` rules and the workspace.

use super::path;
use super::shell::{self, Segment, Word, DYN};
use super::{CmdPat, Context, Entry, Level, Pat, Policy, PosTok, Reason, Verdict};

const MAX_DEPTH: usize = 6;
const SHOWN: usize = 200;

const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "mksh", "ash", "fish", "csh", "tcsh", "busybox",
];
const INTERPRETERS: &[&str] = &[
    "python", "python2", "python3", "pypy", "pypy3", "node", "nodejs", "deno", "bun", "perl",
    "ruby", "php", "lua", "luajit", "tclsh", "osascript", "pwsh", "powershell", "rscript",
    "julia",
];
/// Fetch a URL; their output must not become a script. Forbidden pipe patterns add more.
const FETCHERS: &[&str] = &["curl", "wget", "fetch", "http", "https", "xh", "aria2c"];
/// Run in the shell itself, harmless on their own.
const BUILTINS: &[&str] = &[
    "true", "false", ":", "test", "[", "[[", "]]", "echo", "printf", "pwd", "exit", "return",
    "break", "continue", "shift", "wait", "read", "set", "unset", "shopt", "type", "hash",
    "help", "jobs", "times", "umask", "ulimit", "getopts", "let", "cd", "pushd", "popd", "dirs",
    "export", "local", "declare", "typeset", "readonly",
];
/// Words that open or close a compound command; the command follows them.
const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "!", "{", "}", "esac",
];
/// Variables that change what a later command runs or loads.
const RISKY_VARS: &[&str] = &[
    "PATH", "LD_PRELOAD", "LD_LIBRARY_PATH", "BASH_ENV", "ENV", "PROMPT_COMMAND", "IFS",
    "NODE_OPTIONS", "PYTHONSTARTUP", "PYTHONPATH", "PERL5OPT", "PERL5LIB", "RUBYOPT", "GIT_SSH",
    "GIT_SSH_COMMAND", "GIT_EXTERNAL_DIFF", "GIT_PAGER", "PAGER", "EDITOR", "VISUAL",
    "GIT_EDITOR", "GIT_DIR", "GIT_WORK_TREE", "HOME", "ZDOTDIR", "SHELLOPTS", "BASHOPTS", "PS4",
];
/// Every positional argument is written or removed.
const WRITE_ALL: &[&str] = &[
    "rm", "rmdir", "unlink", "shred", "touch", "mkdir", "truncate", "tee", "mv", "chflags",
    "xattr", "setfacl",
];
/// The first positional is a mode or owner, the rest are written.
const WRITE_AFTER_FIRST: &[&str] = &["chmod", "chown", "chgrp"];
/// The last positional is the destination.
const WRITE_LAST: &[&str] = &["cp", "ln", "install", "rsync", "scp"];
/// Walk directories, so a parent of a credential path reaches it.
const RECURSIVE: &[&str] = &["find", "fd", "rg", "ag", "ack", "tree", "du", "tar", "zip", "rsync"];
const RECURSIVE_WITH_FLAG: &[&str] = &[
    "grep", "egrep", "fgrep", "ls", "cp", "scp", "chmod", "chown", "chgrp", "zip", "rm",
];

struct Acc {
    level: Level,
    reasons: Vec<Reason>,
    resolved: bool,
    writes: Vec<String>,
    /// Some command in here fetches from the network.
    fetches: bool,
}

impl Acc {
    fn new() -> Acc {
        Acc {
            level: Level::ReadOnly,
            reasons: Vec::new(),
            resolved: true,
            writes: Vec::new(),
            fetches: false,
        }
    }

    fn raise(&mut self, level: Level, code: &'static str, seg: &str, detail: Option<String>) {
        if !self.reasons.iter().any(|r| r.code == code && r.segment == seg && r.detail == detail) {
            self.reasons.push(Reason {
                level,
                code,
                segment: seg.to_string(),
                detail,
            });
        }
        self.level = self.level.max(level);
    }

    /// Raises to `sensitive` and marks the verdict as not statically resolved.
    fn unresolved(&mut self, code: &'static str, seg: &str, detail: Option<String>) {
        self.resolved = false;
        self.raise(Level::Sensitive, code, seg, detail);
    }

    fn merge(&mut self, o: Acc) {
        self.level = self.level.max(o.level);
        self.resolved &= o.resolved;
        self.fetches |= o.fetches;
        for r in o.reasons {
            if !self.reasons.iter().any(|x| x.code == r.code && x.segment == r.segment && x.detail == r.detail) {
                self.reasons.push(r);
            }
        }
        for w in o.writes {
            if !self.writes.contains(&w) {
                self.writes.push(w);
            }
        }
    }
}

/// A command after wrappers are removed: what patterns are matched against.
#[derive(Debug, Default)]
struct View {
    name: String,
    short: Vec<char>,
    long: Vec<String>,
    pos: Vec<String>,
}

/// Where a segment's stdin comes from, for interpreters reading a script from it.
#[derive(Clone, Copy, Default)]
struct Input<'s> {
    piped: bool,
    /// An earlier command in the pipeline fetches from the network.
    upstream_fetch: bool,
    seg: Option<&'s Segment>,
}

struct Walker<'a, 'c> {
    policy: &'a Policy,
    ctx: &'a Context<'c>,
    forbidden_paths: Vec<String>,
    roots: Vec<String>,
    fetchers: Vec<String>,
}

fn walker<'a, 'c>(policy: &'a Policy, ctx: &'a Context<'c>) -> Walker<'a, 'c> {
    let fold = |s: &str| if ctx.case_insensitive { s.to_lowercase() } else { s.to_string() };
    let home = ctx.home.as_deref().map(|h| h.trim_end_matches('/').to_string());
    let mut forbidden_paths = Vec::new();
    for p in &policy.forbidden_paths {
        let mut p = p.clone();
        if p.contains("{tokage_app_data}") {
            let Some(a) = &ctx.app_data else { continue };
            p = p.replace("{tokage_app_data}", a.trim_end_matches('/'));
        }
        if p.contains('%') && !cfg!(windows) {
            continue;
        }
        if let (Some(h), true) = (&home, p == "~" || p.starts_with("~/")) {
            p = format!("{h}{}", &p[1..]);
        }
        forbidden_paths.push(fold(&p));
    }
    let roots = ctx
        .workspace_roots
        .iter()
        .filter_map(|r| path::normalize("/", r))
        .map(|r| fold(&r))
        .collect();
    let mut fetchers: Vec<String> = FETCHERS.iter().map(|s| s.to_string()).collect();
    for f in &policy.fetchers {
        if !fetchers.contains(f) {
            fetchers.push(f.clone());
        }
    }
    Walker {
        policy,
        ctx,
        forbidden_paths,
        roots,
        fetchers,
    }
}

fn verdict(acc: Acc, ctx: &Context) -> Verdict {
    let recoverable = match (ctx.tracked, acc.writes.is_empty()) {
        (Some(t), false) => Some(acc.writes.iter().all(|p| t(p))),
        _ => None,
    };
    Verdict {
        level: acc.level,
        reasons: acc.reasons,
        resolved: acc.resolved,
        writes: acc.writes,
        recoverable,
    }
}

pub(super) fn run(policy: &Policy, command: &str, ctx: &Context) -> Verdict {
    let acc = walker(policy, ctx).walk(command, 0, Some(ctx.cwd.clone()));
    verdict(acc, ctx)
}

/// A file write by the CLI's own edit tool (no shell involved). Same path checks as a shell
/// write; a plain edit inside the workspace and `write_scope` is `project`.
pub(super) fn edit(policy: &Policy, file: &str, ctx: &Context) -> Verdict {
    let mut acc = Acc::new();
    let shown = shown(file);
    if file.trim().is_empty() {
        acc.unresolved("empty_path", &shown, None);
    } else {
        walker(policy, ctx).check_write(&Word::literal(file), &Some(ctx.cwd.clone()), &shown, true, &mut acc);
    }
    if acc.level == Level::ReadOnly {
        acc.raise(Level::Project, "edit", &shown, None);
    }
    verdict(acc, ctx)
}

fn shown(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= SHOWN {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(SHOWN).collect::<String>())
    }
}

fn seg_text(words: &[Word]) -> String {
    shown(&words.iter().map(|w| w.raw.as_str()).collect::<Vec<_>>().join(" "))
}

fn is_assignment(w: &Word) -> bool {
    let raw = &w.raw;
    let Some(eq) = raw.find('=') else { return false };
    let name = raw[..eq].trim_end_matches('+');
    let name = name.split('[').next().unwrap_or(name);
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn var_name(w: &Word) -> &str {
    let t = &w.text;
    let n = t.split('=').next().unwrap_or(t).trim_end_matches('+');
    n.split('[').next().unwrap_or(n)
}

fn is_risky_var(name: &str) -> bool {
    RISKY_VARS.contains(&name) || name.starts_with("DYLD_") || name.starts_with("GIT_CONFIG")
}

fn basename(w: &Word) -> String {
    path::basename(&w.text).to_string()
}

/// Splits a value-taking-option command line: returns the index of the first positional.
/// `takes` lists options that consume the next word; glued forms (`-n5`, `--opt=v`) consume
/// nothing extra.
fn skip_opts(args: &[Word], takes: &[&str]) -> usize {
    let mut i = 0;
    while i < args.len() {
        let t = &args[i].text;
        if t == "--" {
            return i + 1;
        }
        if !t.starts_with('-') || t == "-" {
            return i;
        }
        if takes.contains(&t.as_str()) {
            i += 1;
        }
        i += 1;
    }
    i
}

fn split_flags(args: &[Word]) -> (Vec<char>, Vec<String>, Vec<&Word>) {
    let mut short = Vec::new();
    let mut long = Vec::new();
    let mut pos = Vec::new();
    let mut rest = false;
    for w in args {
        let t = &w.text;
        if rest || !t.starts_with('-') || t == "-" || !w.is_static() && t.starts_with(DYN) {
            pos.push(w);
        } else if t == "--" {
            rest = true;
        } else if let Some(l) = t.strip_prefix("--") {
            long.push(format!("--{}", l.split('=').next().unwrap_or(l)));
        } else {
            short.extend(t[1..].chars());
        }
    }
    (short, long, pos)
}

impl<'a, 'c> Walker<'a, 'c> {
    fn fold(&self, s: &str) -> String {
        if self.ctx.case_insensitive {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    }

    fn walk(&self, src: &str, depth: usize, cwd: Option<String>) -> Acc {
        let mut acc = Acc::new();
        if depth > MAX_DEPTH {
            acc.unresolved("too_deep", &shown(src), None);
            return acc;
        }
        if src == DYN.to_string() {
            acc.unresolved("unreadable", "", None);
            return acc;
        }
        let parsed = shell::parse(src);
        if parsed.error {
            acc.unresolved("parse_error", &shown(src), None);
        }
        if parsed.function_def {
            acc.unresolved("function_definition", &shown(src), None);
        }
        // One cwd per subshell level, so `(cd /x && …); rm y` resolves `y` in the outer cwd.
        let mut cwds = vec![cwd];
        // Matches of forbidden pipe patterns' left sides in the current pipeline.
        let mut pipe_lefts: Vec<usize> = Vec::new();
        let mut upstream_fetch = false;
        for seg in &parsed.segments {
            while cwds.len() > seg.depth + 1 {
                cwds.pop();
            }
            while cwds.len() < seg.depth + 1 {
                let top = cwds.last().cloned().flatten();
                cwds.push(top);
            }
            if !seg.piped {
                pipe_lefts.clear();
                upstream_fetch = false;
            }
            let input = Input {
                piped: seg.piped,
                upstream_fetch,
                seg: Some(seg),
            };
            let cwd = cwds.last_mut().expect("one level at least");
            let (sub, view) = self.segment(seg, depth, cwd, input);
            upstream_fetch |= sub.fetches;
            acc.merge(sub);
            if let Some(v) = view {
                self.pipe_rules(&v, &mut pipe_lefts, seg, &mut acc);
            }
        }
        acc
    }

    /// Generic `a | b` patterns: `a` earlier in the pipeline, `b` now.
    fn pipe_rules(&self, v: &View, lefts: &mut Vec<usize>, seg: &Segment, acc: &mut Acc) {
        for (i, e) in self.policy.entries.iter().enumerate() {
            let Pat::Pipe(l, r) = &e.pat else { continue };
            if !e.applies(self.ctx) {
                continue;
            }
            if lefts.contains(&i) && self.match_cmd(r, v, e.level) {
                acc.raise(e.level, "rule", &seg_text(&seg.words), Some(e.text.clone()));
            }
            if self.match_cmd(l, v, e.level) && !lefts.contains(&i) {
                lefts.push(i);
            }
        }
    }

    fn segment(&self, seg: &Segment, depth: usize, cwd: &mut Option<String>, input: Input) -> (Acc, Option<View>) {
        let mut acc = Acc::new();
        let text = seg_text(&seg.words);
        // Substitutions run first, in a subshell.
        let mut sub_fetch = false;
        let subs = seg
            .words
            .iter()
            .chain(seg.redirects.iter().map(|r| &r.target))
            .flat_map(|w| w.subs.iter())
            .chain(seg.stdin_subs.iter());
        for s in subs {
            let a = self.walk(s, depth + 1, cwd.clone());
            sub_fetch |= a.fetches;
            acc.merge(a);
        }
        for r in &seg.redirects {
            match r.op {
                ">" | ">>" | ">|" | "&>" | "&>>" | "<>" => self.check_write(&r.target, cwd, &text, false, &mut acc),
                "<" => self.check_read(&r.target, cwd, &text, false, &mut acc),
                _ => {}
            }
        }
        let view = self.command(&seg.words, depth, cwd, input, sub_fetch, &mut acc);
        (acc, view)
    }

    /// Classifies one simple command (already split into words) into `acc`.
    fn command(
        &self,
        words: &[Word],
        depth: usize,
        cwd: &mut Option<String>,
        input: Input,
        sub_fetch: bool,
        acc: &mut Acc,
    ) -> Option<View> {
        let text = seg_text(words);
        let mut words: Vec<Word> = words.to_vec();
        let mut extra_dynamic = false;
        for _ in 0..16 {
            // Leading assignments: `A=b cmd`.
            let n = words.iter().take_while(|w| is_assignment(w)).count();
            for w in &words[..n] {
                if is_risky_var(var_name(w)) && n < words.len() {
                    acc.raise(Level::Sensitive, "env_override", &text, Some(var_name(w).to_string()));
                }
                self.check_value(w, cwd, &text, acc);
            }
            words.drain(..n);
            let first = words.first()?;
            if first.is_static() && KEYWORDS.contains(&first.text.as_str()) {
                words.remove(0);
                continue;
            }
            if first.is_static() && matches!(first.text.as_str(), "for" | "select" | "case") {
                // Loop lists and case subjects are only values.
                for w in &words[1..] {
                    self.check_read(w, cwd, &text, false, acc);
                }
                return None;
            }
            if first.is_static() && first.text == "time" {
                words.drain(..1 + skip_opts(&words[1..], &["-o", "-f"]));
                continue;
            }
            match self.unwrap(&words) {
                Unwrap::Inner(skip, dynamic) => {
                    words.drain(..skip);
                    extra_dynamic |= dynamic;
                    if words.is_empty() {
                        return None;
                    }
                }
                Unwrap::ReadOnly => {
                    acc.raise(Level::ReadOnly, "builtin", &text, None);
                    return None;
                }
                Unwrap::Done => break,
            }
        }
        if extra_dynamic {
            let mut w = Word::literal("");
            w.text.push(DYN);
            w.dynamic = true;
            w.raw = "…".into();
            words.push(w);
        }
        let name_word = &words[0];
        let args = &words[1..];

        // The command name itself.
        if !name_word.is_static() {
            let mut name = basename(name_word).replace(DYN, "*");
            name = self.fold(&name);
            let forbidden = name.chars().any(|c| c != '*')
                && self.policy.entries.iter().any(|e| {
                    e.level == Level::Forbidden
                        && e.applies(self.ctx)
                        && matches!(&e.pat, Pat::Cmd(p) if path::seg_glob(&name, &self.fold(&p.name)))
                });
            if forbidden {
                acc.raise(Level::Forbidden, "dynamic_command", &text, Some(name));
            } else {
                acc.unresolved("dynamic_command", &text, None);
            }
            for w in args {
                self.check_read(w, cwd, &text, false, acc);
            }
            return None;
        }
        let name = self.fold(&basename(name_word));
        if self.fetchers.iter().any(|f| self.fold(f) == name) {
            acc.fetches = true;
        }
        let view = self.view(&name, args);

        // Base level from the lists.
        let user_override = self.base_level(&view, &text, acc);

        // Command-specific checks.
        let recursive = self.special(&view, args, depth, cwd, input, sub_fetch, &text, user_override, acc);

        // Every argument (and the name) as a path that may be read.
        let quiet = matches!(name.as_str(), "echo" | "printf");
        for w in std::iter::once(name_word).chain(args) {
            if quiet && w.is_static() {
                continue;
            }
            self.check_read(w, cwd, &text, recursive, acc);
        }
        if recursive && view.pos.iter().all(|p| p.starts_with('-')) {
            self.check_read(&Word::literal("."), cwd, &text, true, acc);
        }
        // `cd` moves the cwd for later segments.
        if matches!(name.as_str(), "cd" | "pushd" | "popd") {
            let (_, _, pos) = split_flags(args);
            *cwd = match (name.as_str(), pos.first()) {
                ("popd", _) => None,
                (_, None) => self.ctx.home.clone(),
                (_, Some(w)) if w.is_static() && w.text != "-" => self.resolve(&w.text, w.tilde, cwd),
                _ => None,
            };
        }
        Some(view)
    }

    fn view(&self, name: &str, args: &[Word]) -> View {
        let mut args = args;
        let mut git_config = false;
        if name == "git" {
            // Global options before the subcommand.
            let mut i = 0;
            while i < args.len() {
                let t = args[i].text.as_str();
                match t {
                    "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path" | "--config-env" => {
                        git_config |= t == "-c" || t == "--exec-path" || t == "--config-env";
                        i += 2;
                    }
                    t if t.starts_with("--") && t.contains('=') => {
                        git_config |= t.starts_with("--exec-path") || t.starts_with("--config-env");
                        i += 1;
                    }
                    t if t.starts_with('-') && t.len() > 1 => i += 1,
                    _ => break,
                }
            }
            args = &args[i.min(args.len())..];
        }
        let (short, mut long, pos) = split_flags(args);
        let mut pos: Vec<String> = pos.iter().map(|w| w.text.clone()).collect();
        if git_config {
            long.push("--tokage-git-config".into());
        }
        if name == "git" && pos.first().map(String::as_str) == Some("push") {
            // `+ref` and `:ref` refspecs force-push or delete; `--mirror` implies force.
            let mut force = long.iter().any(|l| l == "--mirror");
            for p in pos.iter_mut().skip(2) {
                if let Some(r) = p.strip_prefix('+') {
                    force = true;
                    *p = r.to_string();
                } else if p.starts_with(':') {
                    force = true;
                }
            }
            if force && !long.iter().any(|l| l == "--force") {
                long.push("--force".into());
            }
            let mut short = short;
            if force && !short.contains(&'f') {
                short.push('f');
            }
            return View {
                name: name.into(),
                short,
                long,
                pos,
            };
        }
        View {
            name: name.into(),
            short,
            long,
            pos,
        }
    }

    fn name_eq(&self, a: &str, b: &str) -> bool {
        if self.ctx.case_insensitive {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    }

    fn match_cmd(&self, p: &CmdPat, v: &View, level: Level) -> bool {
        if !self.name_eq(&p.name, &v.name) {
            return false;
        }
        let has = |f: &str| {
            if f.starts_with("--") {
                v.long.iter().any(|l| l == f)
            } else {
                f[1..].chars().all(|c| v.short.contains(&c))
            }
        };
        if !p.flags.iter().all(|f| has(f)) {
            return false;
        }
        for (i, t) in p.pos.iter().enumerate() {
            match t {
                PosTok::Lit(s) => {
                    if v.pos.get(i) != Some(s) {
                        return false;
                    }
                }
                PosTok::DefaultBranch => return self.hits_default_branch(&v.pos[i.min(v.pos.len())..], v),
            }
        }
        // An allowing pattern with flags (`node --version`) allows exactly that: no other
        // flags or arguments, which could turn it into something else.
        if level <= Level::Project && !p.flags.is_empty() {
            let pshort: Vec<char> = p.flags.iter().filter(|f| !f.starts_with("--")).flat_map(|f| f[1..].chars()).collect();
            if v.pos.len() != p.pos.len()
                || !v.short.iter().all(|c| pshort.contains(c))
                || !v.long.iter().all(|l| p.flags.contains(l))
            {
                return false;
            }
        }
        true
    }

    /// `rest` is `[remote, refspec…]` of a push.
    fn hits_default_branch(&self, rest: &[String], v: &View) -> bool {
        let defaults: Vec<&str> = match &self.ctx.default_branch {
            Some(b) => vec![b.as_str()],
            None => vec!["main", "master"],
        };
        let current_is_default = self.ctx.current_branch.as_deref().map_or(true, |c| defaults.contains(&c));
        if v.long.iter().any(|l| l == "--all" || l == "--mirror" || l == "--branches") {
            return true;
        }
        if rest.len() <= 1 {
            return current_is_default;
        }
        rest[1..].iter().any(|r| {
            let dst = r.rsplit(':').next().unwrap_or(r);
            let dst = dst.strip_prefix("refs/heads/").unwrap_or(dst);
            if dst == "HEAD" || dst.is_empty() || dst.contains(DYN) {
                return current_is_default || dst.contains(DYN);
            }
            defaults.contains(&dst)
        })
    }

    /// Sets the level from the lists. Returns whether a user rule decided it.
    fn base_level(&self, v: &View, text: &str, acc: &mut Acc) -> bool {
        let mut pack: Option<&Entry> = None;
        let mut user: Option<&Entry> = None;
        let mut forbidden: Option<&Entry> = None;
        let spec = |e: &Entry| match &e.pat {
            Pat::Cmd(c) => c.specificity(),
            _ => 0,
        };
        for e in &self.policy.entries {
            let Pat::Cmd(p) = &e.pat else { continue };
            if !e.applies(self.ctx) || !self.match_cmd(p, v, e.level) {
                continue;
            }
            if e.level == Level::Forbidden {
                forbidden.get_or_insert(e);
            } else if e.user {
                if user.map_or(true, |u| (spec(e), e.level) > (spec(u), u.level)) {
                    user = Some(e);
                }
            } else if pack.map_or(true, |p| e.level > p.level) {
                pack = Some(e);
            }
        }
        if let Some(e) = forbidden {
            acc.raise(Level::Forbidden, "rule", text, Some(e.text.clone()));
            return false;
        }
        if let Some(e) = user {
            acc.raise(e.level, "user_rule", text, Some(e.text.clone()));
            return true;
        }
        if let Some(e) = pack {
            acc.raise(e.level, "rule", text, Some(e.text.clone()));
        } else if BUILTINS.contains(&v.name.as_str()) {
            acc.raise(Level::ReadOnly, "builtin", text, None);
        } else {
            acc.raise(Level::Sensitive, "unknown_command", text, Some(v.name.clone()));
        }
        false
    }

    /// Wrappers that run the rest of their arguments as a command.
    fn unwrap(&self, words: &[Word]) -> Unwrap {
        let w = &words[0];
        if !w.is_static() {
            return Unwrap::Done;
        }
        let name = self.fold(&basename(w));
        let args = &words[1..];
        let inner = |takes: &[&str]| Unwrap::Inner(1 + skip_opts(args, takes), false);
        match name.as_str() {
            "command" => {
                if args.iter().take_while(|a| a.text.starts_with('-')).any(|a| a.text.contains(['v', 'V'])) {
                    Unwrap::ReadOnly
                } else {
                    inner(&[])
                }
            }
            "builtin" | "nohup" | "setsid" => inner(&[]),
            "exec" => inner(&["-a"]),
            "nice" => inner(&["-n"]),
            "stdbuf" => inner(&["-i", "-o", "-e"]),
            "caffeinate" => inner(&["-t", "-w"]),
            "arch" => inner(&["-e"]),
            "timeout" => {
                let i = skip_opts(args, &["-s", "-k", "--signal", "--kill-after"]);
                Unwrap::Inner(1 + (i + 1).min(args.len()), false)
            }
            "xargs" => {
                let i = skip_opts(args, &["-I", "-n", "-P", "-d", "-L", "-s", "-E", "-a", "-R", "-S", "-J", "--delimiter", "--max-args", "--max-procs", "--arg-file"]);
                if i >= args.len() {
                    // Plain `xargs` runs `echo`.
                    Unwrap::ReadOnly
                } else {
                    Unwrap::Inner(1 + i, true)
                }
            }
            "env" => {
                let mut i = 0;
                while i < args.len() {
                    let t = args[i].text.as_str();
                    if t == "--" {
                        i += 1;
                        break;
                    }
                    if matches!(t, "-u" | "-C" | "-P" | "--unset" | "--chdir") {
                        i += 2;
                    } else if matches!(t, "-S" | "--split-string") || t.starts_with("--split-string=") {
                        // A command line packed into one word: not followed further.
                        return Unwrap::Done;
                    } else if (t.starts_with('-') && t != "-") || is_assignment(&args[i]) {
                        i += 1;
                    } else {
                        break;
                    }
                }
                if i >= args.len() {
                    Unwrap::Done
                } else {
                    // Assignments stay as leading words of the inner command.
                    let skip = 1 + args[..i].iter().rposition(|a| !is_assignment(a)).map_or(0, |k| k + 1);
                    Unwrap::Inner(skip, false)
                }
            }
            _ => Unwrap::Done,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn special(
        &self,
        v: &View,
        args: &[Word],
        depth: usize,
        cwd: &mut Option<String>,
        input: Input,
        sub_fetch: bool,
        text: &str,
        user_override: bool,
        acc: &mut Acc,
    ) -> bool {
        let name = v.name.as_str();
        let (_, _, pos) = split_flags(args);
        let flag_value = |names: &[&str]| -> Vec<Word> {
            let mut out = Vec::new();
            for (i, a) in args.iter().enumerate() {
                let t = a.text.as_str();
                for n in names {
                    if t == *n {
                        if let Some(next) = args.get(i + 1) {
                            out.push(next.clone());
                        }
                    } else if n.starts_with("--") && t.starts_with(&format!("{n}=")) {
                        let mut w = a.clone();
                        w.text = t[n.len() + 1..].to_string();
                        w.tilde = w.text.starts_with('~');
                        out.push(w);
                    } else if !n.starts_with("--") && t.starts_with(n) && t.len() > n.len() && !t.starts_with("--") {
                        let mut w = a.clone();
                        w.text = t[n.len()..].to_string();
                        w.tilde = false;
                        out.push(w);
                    }
                }
            }
            out
        };
        let mut recursive = RECURSIVE.contains(&name)
            || (RECURSIVE_WITH_FLAG.contains(&name)
                && (v.short.contains(&'r') || v.short.contains(&'R') || v.short.contains(&'a') && matches!(name, "cp" | "scp")
                    || v.long.iter().any(|l| l == "--recursive" || l == "--archive")));
        let write = |w: &Word, acc: &mut Acc| self.check_write(w, cwd, text, user_override, acc);

        let is_shell = SHELLS.contains(&name);
        if is_shell || INTERPRETERS.contains(&name) || matches!(name, "eval" | "source" | ".") {
            // Something that runs code. Fetched code reaching it is forbidden.
            let fetched = sub_fetch || (input.piped && input.upstream_fetch && !self.has_script_arg(name, &pos));
            if fetched {
                acc.raise(Level::Forbidden, "remote_script", text, None);
            }
        }
        if is_shell {
            let inline = v.short.contains(&'c') || v.long.iter().any(|l| l == "--command");
            if inline {
                let script = pos.first();
                acc.raise(Level::Sensitive, "inline_interpreter", text, None);
                match script {
                    Some(s) if s.is_static() => {
                        let inner = self.walk(&s.text, depth + 1, cwd.clone());
                        acc.merge(inner);
                    }
                    _ => acc.unresolved("inline_interpreter", text, None),
                }
            } else if pos.is_empty() || pos[0].text == "-" {
                let bodies = input.seg.map(|s| s.stdin.clone()).unwrap_or_default();
                if !bodies.is_empty() && !input.piped {
                    acc.raise(Level::Sensitive, "inline_interpreter", text, None);
                    for b in bodies {
                        let inner = self.walk(&b, depth + 1, cwd.clone());
                        acc.merge(inner);
                    }
                } else if input.piped {
                    acc.unresolved("stdin_script", text, None);
                }
            }
        } else if INTERPRETERS.contains(&name) {
            let inline = v.short.iter().any(|c| matches!(c, 'c' | 'e' | 'E'))
                || (name == "php" && v.short.contains(&'r'))
                || (matches!(name, "node" | "nodejs" | "bun") && v.short.contains(&'p'))
                || v.long.iter().any(|l| matches!(l.as_str(), "--eval" | "--print" | "--command"))
                || (name == "deno" && pos.first().is_some_and(|p| p.text == "eval"));
            if inline {
                acc.raise(Level::Sensitive, "inline_interpreter", text, None);
            } else if pos.is_empty() || pos[0].text == "-" {
                let heredoc = input.seg.is_some_and(|s| !s.stdin.is_empty());
                if input.piped || heredoc {
                    acc.unresolved("stdin_script", text, None);
                }
            }
        }
        match name {
            "eval" => {
                acc.raise(Level::Sensitive, "inline_interpreter", text, None);
                if args.iter().all(Word::is_static) {
                    let src = args.iter().map(|a| a.text.as_str()).collect::<Vec<_>>().join(" ");
                    let inner = self.walk(&src, depth + 1, cwd.clone());
                    acc.merge(inner);
                } else {
                    acc.unresolved("inline_interpreter", text, None);
                }
            }
            "trap" => {
                if let Some(s) = pos.first() {
                    if s.is_static() {
                        let inner = self.walk(&s.text, depth + 1, cwd.clone());
                        acc.merge(inner);
                    } else {
                        acc.unresolved("inline_interpreter", text, None);
                    }
                }
            }
            "watch" => {
                let i = skip_opts(args, &["-n", "-d", "--interval"]);
                let rest = &args[i.min(args.len())..];
                if rest.iter().all(Word::is_static) {
                    let src = rest.iter().map(|a| a.text.as_str()).collect::<Vec<_>>().join(" ");
                    let inner = self.walk(&src, depth + 1, cwd.clone());
                    acc.merge(inner);
                } else {
                    acc.unresolved("inline_interpreter", text, None);
                }
            }
            "alias" => {
                if pos.iter().any(|p| p.text.contains('=')) {
                    acc.unresolved("alias", text, None);
                }
            }
            "export" | "declare" | "typeset" | "local" | "readonly" => {
                for w in &pos {
                    if w.text.contains('=') && is_risky_var(var_name(w)) {
                        acc.raise(Level::Sensitive, "env_override", text, Some(var_name(w).to_string()));
                    }
                }
            }
            "ps" => {
                // BSD-style `e`, and on macOS `-e` / `-E`, print other processes' environment.
                let bsd = pos.iter().any(|p| p.text.chars().all(|c| c.is_ascii_alphabetic()) && p.text.contains(['e', 'E']));
                let dash = cfg!(target_os = "macos") && v.short.iter().any(|c| matches!(c, 'e' | 'E'));
                if bsd || dash {
                    acc.raise(Level::Forbidden, "process_env", text, None);
                }
            }
            "git" => {
                if v.long.iter().any(|l| l == "--tokage-git-config") {
                    acc.raise(Level::Sensitive, "git_config", text, None);
                }
                match v.pos.first().map(String::as_str) {
                    Some("branch") => {
                        let modifies = v.short.iter().any(|c| "dDmMcCfu".contains(*c))
                            || v.long.iter().any(|l| {
                                matches!(
                                    l.as_str(),
                                    "--delete" | "--move" | "--copy" | "--force" | "--set-upstream-to" | "--unset-upstream" | "--edit-description" | "--track"
                                )
                            });
                        let listing = v.short.contains(&'l') || v.long.iter().any(|l| l == "--list");
                        if modifies || (v.pos.len() > 1 && !listing) {
                            acc.raise(Level::Sensitive, "git_branch_write", text, None);
                        }
                    }
                    Some("diff" | "log" | "show" | "format-patch") => {
                        for w in flag_value(&["--output"]) {
                            write(&w, acc);
                        }
                        if v.long.iter().any(|l| l == "--ext-diff") {
                            acc.raise(Level::Sensitive, "executes_command", text, Some("--ext-diff".into()));
                        }
                    }
                    _ => {}
                }
            }
            "find" => {
                let starts: Vec<&Word> = args.iter().take_while(|a| !a.text.starts_with(['-', '(', '!']) || a.text == "-").collect();
                let start_glob = |w: Option<&Word>| -> Word {
                    let mut g = w.cloned().unwrap_or_else(|| Word::literal("."));
                    g.text = format!("{}/*", g.text.trim_end_matches('/'));
                    g.glob = true;
                    g
                };
                let mut i = 0;
                while i < args.len() {
                    let t = args[i].text.as_str();
                    match t {
                        "-exec" | "-execdir" | "-ok" | "-okdir" => {
                            let end = args[i + 1..].iter().position(|a| a.text == ";" || a.text == "+").map_or(args.len(), |k| i + 1 + k);
                            let mut nested: Vec<Word> = args[i + 1..end].to_vec();
                            for w in nested.iter_mut() {
                                if w.text.contains("{}") {
                                    let g = start_glob(starts.first().copied());
                                    w.text = w.text.replace("{}", &g.text);
                                    w.glob = true;
                                }
                            }
                            if !nested.is_empty() {
                                let mut inner = Acc::new();
                                self.command(&nested, depth + 1, &mut cwd.clone(), Input::default(), false, &mut inner);
                                acc.merge(inner);
                            }
                            i = end + 1;
                            continue;
                        }
                        "-delete" => {
                            acc.raise(Level::Sensitive, "deletes_files", text, None);
                            if starts.is_empty() {
                                write(&start_glob(None), acc);
                            }
                            for s in &starts {
                                write(&start_glob(Some(s)), acc);
                            }
                        }
                        "-fprint" | "-fprint0" | "-fprintf" | "-fls" => {
                            if let Some(w) = args.get(i + 1) {
                                write(w, acc);
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            "fd" | "fdfind" => {
                if let Some(i) = args.iter().position(|a| matches!(a.text.as_str(), "-x" | "--exec" | "-X" | "--exec-batch")) {
                    let end = args[i + 1..].iter().position(|a| a.text == ";").map_or(args.len(), |k| i + 1 + k);
                    let mut nested: Vec<Word> = args[i + 1..end].to_vec();
                    for w in nested.iter_mut() {
                        if w.text.contains('{') {
                            w.text = "./*".into();
                            w.glob = true;
                        }
                    }
                    if nested.len() == 1 {
                        let mut g = Word::literal("./*");
                        g.glob = true;
                        nested.push(g);
                    }
                    if !nested.is_empty() {
                        let mut inner = Acc::new();
                        self.command(&nested, depth + 1, &mut cwd.clone(), Input::default(), false, &mut inner);
                        acc.merge(inner);
                    }
                }
            }
            "rg" => {
                for w in flag_value(&["--pre"]) {
                    acc.raise(Level::Sensitive, "executes_command", text, Some("--pre".into()));
                    let mut inner = Acc::new();
                    self.command(&[w, Word::literal("./x")], depth + 1, &mut cwd.clone(), Input::default(), false, &mut inner);
                    acc.merge(inner);
                }
            }
            "sort" => {
                for w in flag_value(&["-o", "--output"]) {
                    write(&w, acc);
                }
                if v.long.iter().any(|l| l == "--compress-program") {
                    acc.raise(Level::Sensitive, "executes_command", text, Some("--compress-program".into()));
                }
            }
            "tree" => {
                for w in flag_value(&["-o"]) {
                    write(&w, acc);
                }
            }
            "uniq" => {
                if let Some(w) = pos.get(1) {
                    write(w, acc);
                }
            }
            "date" => {
                if v.short.contains(&'s') || v.long.iter().any(|l| l == "--set") {
                    acc.raise(Level::Sensitive, "sets_clock", text, None);
                }
            }
            "sed" | "perl" => {
                if v.short.contains(&'i') || v.long.iter().any(|l| l == "--in-place") {
                    let script_given = v.short.contains(&'e') || v.short.contains(&'f') || v.long.iter().any(|l| l == "--expression" || l == "--file");
                    for w in pos.iter().skip(if script_given { 0 } else { 1 }) {
                        write(w, acc);
                    }
                }
            }
            "dd" => {
                for a in args {
                    if let Some(rest) = a.text.strip_prefix("of=") {
                        let mut w = a.clone();
                        w.text = rest.to_string();
                        w.tilde = false;
                        write(&w, acc);
                    }
                }
            }
            "curl" => {
                for w in flag_value(&["-o", "--output"]) {
                    write(&w, acc);
                }
                if v.short.contains(&'O') || v.long.iter().any(|l| l == "--remote-name" || l == "--remote-name-all") {
                    let mut g = Word::literal("./*");
                    g.glob = true;
                    write(&g, acc);
                }
            }
            "wget" => {
                for w in flag_value(&["-O", "--output-document", "-P", "--directory-prefix"]) {
                    write(&w, acc);
                }
            }
            "tar" | "bsdtar" | "gtar" => {
                let extract = v.short.contains(&'x') || v.long.iter().any(|l| l == "--extract")
                    || pos.first().is_some_and(|p| !p.text.contains('/') && p.text.contains('x') && p.text.chars().all(|c| c.is_ascii_alphabetic()));
                if extract {
                    let dirs = flag_value(&["-C", "--directory"]);
                    if dirs.is_empty() {
                        let mut g = Word::literal("./*");
                        g.glob = true;
                        write(&g, acc);
                    }
                    for d in dirs {
                        write(&d, acc);
                    }
                }
            }
            "unzip" => {
                let dirs = flag_value(&["-d"]);
                if dirs.is_empty() {
                    let mut g = Word::literal("./*");
                    g.glob = true;
                    write(&g, acc);
                }
                for d in dirs {
                    write(&d, acc);
                }
            }
            _ => {}
        }
        if WRITE_ALL.contains(&name) || WRITE_AFTER_FIRST.contains(&name) || WRITE_LAST.contains(&name) {
            let targets = flag_value(&["-t", "--target-directory"]);
            let skip = usize::from(WRITE_AFTER_FIRST.contains(&name) && !v.long.iter().any(|l| l == "--reference"));
            let local = |w: &Word| {
                // `host:path` is remote for scp / rsync.
                !(matches!(name, "scp" | "rsync") && w.text.split('/').next().is_some_and(|h| h.contains(':')))
            };
            if WRITE_LAST.contains(&name) {
                if targets.is_empty() {
                    if let Some(w) = pos.iter().rfind(|w| local(w)).filter(|_| pos.len() > 1) {
                        write(w, acc);
                    }
                }
            } else {
                for w in pos.iter().skip(skip).filter(|w| local(w)) {
                    write(w, acc);
                }
            }
            for w in &targets {
                write(w, acc);
            }
            if name == "rm" && recursive {
                recursive = true;
            }
        }
        recursive
    }

    /// The interpreter has a script argument, so stdin is data rather than code.
    fn has_script_arg(&self, name: &str, pos: &[&Word]) -> bool {
        !matches!(name, "eval" | "source" | ".") && pos.first().is_some_and(|p| p.text != "-")
    }

    /// Resolves a path word. `None`: relative while the cwd is unknown.
    fn resolve(&self, text: &str, tilde: bool, cwd: &Option<String>) -> Option<String> {
        let mut t = text.to_string();
        if t.starts_with('~') && !tilde {
            t = format!("./{t}");
        }
        if t.starts_with('~') && t != "~" && !t.starts_with("~/") {
            // `~user/…`: another home, same layout.
            t = match t.find('/') {
                Some(i) => format!("~{}", &t[i..]),
                None => "~".into(),
            };
        }
        if let Some(h) = &self.ctx.home {
            if t == "~" || t.starts_with("~/") {
                t = format!("{}{}", h.trim_end_matches('/'), &t[1..]);
            }
        }
        let relative = !t.starts_with('/') && !t.starts_with('~');
        let base = match cwd {
            Some(c) => c.clone(),
            None if relative => return None,
            None => "/".into(),
        };
        let p = path::normalize(&base, &t).unwrap_or_else(|| "/".into());
        Some(self.fold(&p))
    }

    fn inside_workspace(&self, p: &str) -> bool {
        self.roots.iter().any(|r| path::under(p, r))
    }

    fn matches_path(pattern: &str, p: &str, wild: bool) -> bool {
        if wild {
            path::overlap(p, pattern)
        } else {
            path::glob(pattern, p)
        }
    }

    /// Path rules (`read:` / `write:`) from the lists: raises to the deciding level.
    fn path_rules(&self, p: &str, wild: bool, write: bool, text: &str, acc: &mut Acc) {
        let mut user: Option<&Entry> = None;
        let mut pack: Option<&Entry> = None;
        for e in &self.policy.entries {
            let g = match (&e.pat, write) {
                (Pat::Read(g), _) | (Pat::Write(g), true) => g,
                _ => continue,
            };
            if !e.applies(self.ctx) {
                continue;
            }
            let g = self.fold(g);
            let hit = if !g.contains('/') {
                let base = path::basename(p);
                if wild {
                    path::overlap(base, &g)
                } else {
                    path::seg_glob(&g, base)
                }
            } else {
                let full = if g.starts_with('/') {
                    g.clone()
                } else if g == "~" || g.starts_with("~/") {
                    match &self.ctx.home {
                        Some(h) => self.fold(&format!("{}{}", h.trim_end_matches('/'), &g[1..])),
                        None => g.clone(),
                    }
                } else {
                    let root = self.roots.first().cloned().unwrap_or_default();
                    format!("{root}/{}", g.trim_start_matches("./"))
                };
                Self::matches_path(&full, p, wild)
            };
            if !hit {
                continue;
            }
            if e.level == Level::Forbidden {
                acc.raise(Level::Forbidden, "path_rule", text, Some(e.text.clone()));
                return;
            }
            let slot = if e.user { &mut user } else { &mut pack };
            if slot.map_or(true, |s| e.level > s.level) {
                *slot = Some(e);
            }
        }
        if let Some(e) = user.or(pack) {
            acc.raise(e.level, "path_rule", text, Some(e.text.clone()));
        }
    }

    /// Path-like parts of a word: the word itself, `--opt=value`, `-Xvalue`, `@file`.
    fn candidates(w: &Word) -> Vec<(String, bool)> {
        let t = &w.text;
        let mut out = Vec::new();
        let flag = t.starts_with('-') && t != "-";
        if !flag {
            out.push((t.clone(), w.tilde));
        } else if !t.starts_with("--") && t.len() > 2 {
            let v = &t[2..];
            if v.starts_with(['/', '~', '.']) {
                out.push((v.to_string(), v.starts_with('~')));
            }
        }
        for sep in ['=', '@'] {
            if let Some(i) = t.find(sep) {
                let v = &t[i + 1..];
                if !v.is_empty() && (sep == '=' || v.starts_with(['/', '~', '.'])) {
                    out.push((v.to_string(), v.starts_with('~')));
                }
            }
        }
        out.retain(|(s, _)| !s.is_empty() && s != "-");
        out
    }

    fn check_value(&self, w: &Word, cwd: &Option<String>, text: &str, acc: &mut Acc) {
        if let Some(i) = w.text.find('=') {
            let v = &w.text[i + 1..];
            for part in v.split(':') {
                if !part.is_empty() {
                    self.check_path(part, part.starts_with('~'), w.glob || w.dynamic, cwd, text, false, false, acc);
                }
            }
        }
    }

    fn check_read(&self, w: &Word, cwd: &Option<String>, text: &str, recursive: bool, acc: &mut Acc) {
        for (c, tilde) in Self::candidates(w) {
            self.check_path(&c, tilde, w.glob || w.dynamic || w.uncertain, cwd, text, recursive, true, acc);
        }
    }

    /// Credential paths and `read:` rules for one path. `report_dynamic`: an unknown
    /// argument makes the verdict unresolved.
    #[allow(clippy::too_many_arguments)]
    fn check_path(
        &self,
        raw: &str,
        tilde: bool,
        wild: bool,
        cwd: &Option<String>,
        text: &str,
        recursive: bool,
        report_dynamic: bool,
        acc: &mut Acc,
    ) {
        if raw.starts_with(DYN) {
            if report_dynamic {
                acc.unresolved("dynamic_argument", text, None);
            }
            return;
        }
        let spelled = raw.replace(DYN, "*");
        let Some(p) = self.resolve(&spelled, tilde, cwd) else {
            if report_dynamic {
                acc.unresolved("unknown_cwd", text, Some(raw.to_string()));
            }
            return;
        };
        let wild = wild || spelled.contains('*');
        for fp in &self.forbidden_paths {
            if Self::matches_path(fp, &p, wild) {
                acc.raise(Level::Forbidden, "credential_path", text, Some(raw.to_string()));
                return;
            }
        }
        if recursive {
            let base = if wild { path::literal_base(&p) } else { p.clone() };
            if self.forbidden_paths.iter().any(|fp| path::under(&path::literal_base(fp), &base)) {
                acc.raise(Level::Sensitive, "covers_credentials", text, Some(raw.to_string()));
            }
        }
        self.path_rules(&p, wild, false, text, acc);
    }

    fn check_write(&self, w: &Word, cwd: &Option<String>, text: &str, user_override: bool, acc: &mut Acc) {
        let t = &w.text;
        if t.is_empty() {
            return;
        }
        // Credential paths and `read:` rules apply to writes as well.
        self.check_path(t, w.tilde, w.glob || w.dynamic, cwd, text, false, false, acc);
        if w.dynamic || w.uncertain {
            acc.unresolved("dynamic_path", text, Some(w.raw.clone()));
            // The static part before the unknown one still has to stay inside.
            let head = &t[..t.find(DYN).unwrap_or(t.len())];
            let head = &head[..head.rfind('/').map_or(0, |i| i + 1)];
            if !head.is_empty() {
                if let Some(p) = self.resolve(head, w.tilde, cwd) {
                    if !self.inside_workspace(&p) {
                        acc.raise(Level::Forbidden, "outside_workspace", text, Some(w.raw.clone()));
                    }
                }
            }
            return;
        }
        let Some(p) = self.resolve(t, w.tilde, cwd) else {
            acc.unresolved("unknown_cwd", text, Some(w.raw.clone()));
            return;
        };
        if matches!(p.as_str(), "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/tty") || p.starts_with("/dev/fd/") {
            return;
        }
        let mut targets = vec![if w.glob { path::literal_base(&p) } else { p.clone() }];
        if w.glob {
            // `.*` can match `..` in some shells.
            let segs: Vec<&str> = t.split('/').collect();
            if segs.iter().any(|s| s.contains(['*', '?', '[']) && path::seg_glob(s, "..")) {
                let alt = segs.iter().map(|s| if s.contains(['*', '?', '[']) && path::seg_glob(s, "..") { ".." } else { s }).collect::<Vec<_>>().join("/");
                if let Some(a) = self.resolve(&alt, w.tilde, cwd) {
                    targets.push(path::literal_base(&a));
                }
            }
        }
        for target in &targets {
            if !self.inside_workspace(target) {
                acc.raise(Level::Forbidden, "outside_workspace", text, Some(w.raw.clone()));
                return;
            }
        }
        let target = targets.swap_remove(0);
        self.path_rules(&p, w.glob, true, text, acc);
        if let Some(scope) = &self.ctx.write_scope {
            let root = self.roots.iter().find(|r| path::under(&target, r)).cloned().unwrap_or_default();
            let rel = target.strip_prefix(&root).unwrap_or(&target).trim_start_matches('/').to_string();
            let ok = scope.iter().any(|g| {
                let g = self.fold(g);
                path::glob(&g, &rel) || (w.glob && path::under(&rel, &path::literal_base(&g)) && !path::literal_base(&g).is_empty())
            });
            if !ok {
                acc.raise(Level::Sensitive, "outside_write_scope", text, Some(w.raw.clone()));
            }
        }
        if target.split('/').any(|s| s == ".git") {
            acc.raise(Level::Sensitive, "git_internal", text, Some(w.raw.clone()));
        }
        if !user_override {
            acc.raise(Level::Sensitive, "writes_file", text, Some(w.raw.clone()));
        }
        if !acc.writes.contains(&target) {
            acc.writes.push(target);
        }
    }
}

enum Unwrap {
    /// Drop this many words (the wrapper and its options); the flag adds an unknown argument.
    Inner(usize, bool),
    /// The wrapper only reports (`command -v`, plain `xargs`).
    ReadOnly,
    Done,
}
