//! Lexical path handling for the command matcher. Nothing touches the filesystem: symlinks
//! are not resolved (the sandbox in P2.m4 is the boundary for those).

/// Joins `p` onto `cwd` when relative and removes `.` / `..` / repeated slashes. Paths
/// starting with `~` stay relative to `~`; climbing above it gives `None`.
pub fn normalize(cwd: &str, p: &str) -> Option<String> {
    let joined = if p.starts_with('/') || p == "~" || p.starts_with("~/") {
        p.to_string()
    } else {
        format!("{cwd}/{p}")
    };
    let tilde = joined.starts_with('~');
    let mut out: Vec<&str> = Vec::new();
    for (i, seg) in joined.split('/').enumerate() {
        match seg {
            "" | "." => {}
            "~" if i == 0 && tilde => {}
            ".." => {
                if out.pop().is_none() && tilde {
                    return None;
                }
            }
            s => out.push(s),
        }
    }
    let body = out.join("/");
    Some(match (tilde, body.is_empty()) {
        (true, true) => "~".into(),
        (true, false) => format!("~/{body}"),
        (false, _) => format!("/{body}"),
    })
}

/// `path` is `root` or below it.
pub fn under(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        return path.starts_with('/');
    }
    path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

/// `[abc]`, `[a-z]`, `[!x]` / `[^x]` at the start of `p`: whether `c` is in the class, and
/// the class length. `None` when `[` has no closing `]` (then it is a literal).
fn class(p: &[char], c: char) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = matches!(p.get(1), Some('!' | '^'));
    if negate {
        i += 1;
    }
    let start = i;
    let mut hit = false;
    while i < p.len() {
        if p[i] == ']' && i > start {
            return Some((hit != negate, i + 1));
        }
        if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|&e| e != ']') {
            hit |= p[i] <= c && c <= p[i + 2];
            i += 3;
        } else {
            hit |= p[i] == c;
            i += 1;
        }
    }
    None
}

fn seg_match(p: &[char], s: &[char]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some('*') => (0..=s.len()).any(|k| seg_match(&p[1..], &s[k..])),
        Some('?') => !s.is_empty() && seg_match(&p[1..], &s[1..]),
        Some('[') if !s.is_empty() => match class(p, s[0]) {
            Some((hit, len)) => hit && seg_match(&p[len..], &s[1..]),
            None => s[0] == '[' && seg_match(&p[1..], &s[1..]),
        },
        Some(c) => s.first() == Some(c) && seg_match(&p[1..], &s[1..]),
    }
}

/// One path segment against a shell glob segment. A leading `.` is only matched by a
/// literal `.`, as in the shell without `dotglob`.
pub fn seg_glob(pattern: &str, name: &str) -> bool {
    if pattern == "**" {
        return !name.starts_with('.');
    }
    if name.starts_with('.') && !pattern.starts_with('.') {
        return false;
    }
    let pc: Vec<char> = pattern.chars().collect();
    let sc: Vec<char> = name.chars().collect();
    seg_match(&pc, &sc)
}

fn has_wild(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

fn seg_overlap(x: &str, y: &str) -> bool {
    match (has_wild(x), has_wild(y)) {
        (false, false) => x == y,
        (true, false) => seg_glob(x, y),
        (false, true) => seg_glob(y, x),
        // A leading wildcard does not match a leading `.` (no `dotglob`).
        (true, true) => x.starts_with('.') == y.starts_with('.') || !(x.starts_with(['*', '?', '[']) || y.starts_with(['*', '?', '['])),
    }
}

fn segs_overlap(a: &[&str], b: &[&str]) -> bool {
    match (a.first(), b.first()) {
        (None, None) => true,
        (Some(&"**"), _) => segs_overlap(&a[1..], b) || (!b.is_empty() && segs_overlap(a, &b[1..])),
        (_, Some(&"**")) => segs_overlap(b, a),
        (None, _) | (_, None) => false,
        (Some(x), Some(y)) => seg_overlap(x, y) && segs_overlap(&a[1..], &b[1..]),
    }
}

/// Some path could match both globs. `**` matches anything here, dot files included, so
/// the answer errs towards an overlap. A trailing `/**` also covers the directory itself.
pub fn overlap(a: &str, b: &str) -> bool {
    if a.starts_with('/') != b.starts_with('/') {
        return false;
    }
    let (sa, sb) = (split(a), split(b));
    if segs_overlap(&sa, &sb) {
        return true;
    }
    let trim = |s: &[&str]| -> Option<Vec<String>> {
        (s.last() == Some(&"**")).then(|| s[..s.len() - 1].iter().map(|x| x.to_string()).collect())
    };
    let check = |t: Option<Vec<String>>, other: &[&str]| {
        t.is_some_and(|t| segs_overlap(&t.iter().map(String::as_str).collect::<Vec<_>>(), other))
    };
    check(trim(&sa), &sb) || check(trim(&sb), &sa)
}

fn segs_match(p: &[&str], s: &[&str]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(&"**") => (0..=s.len()).any(|k| segs_match(&p[1..], &s[k..])),
        Some(seg) => {
            !s.is_empty() && {
                let pc: Vec<char> = seg.chars().collect();
                let sc: Vec<char> = s[0].chars().collect();
                seg_match(&pc, &sc) && segs_match(&p[1..], &s[1..])
            }
        }
    }
}

fn split(p: &str) -> Vec<&str> {
    p.split('/').filter(|s| !s.is_empty()).collect()
}

/// Glob match over `/` segments: `*` and `?` within a segment, `**` across segments. A
/// trailing `/**` also matches the directory itself.
pub fn glob(pattern: &str, path: &str) -> bool {
    let p = split(pattern);
    let s = split(path);
    segs_match(&p, &s) || (p.last() == Some(&"**") && segs_match(&p[..p.len() - 1], &s))
}

/// The literal leading part of a glob pattern (up to the first segment with a wildcard).
pub fn literal_base(pattern: &str) -> String {
    let lead = if pattern.starts_with('/') { "/" } else { "" };
    let segs: Vec<&str> = split(pattern)
        .into_iter()
        .take_while(|s| !s.contains(['*', '?', '[']))
        .collect();
    format!("{lead}{}", segs.join("/"))
}

pub fn basename(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_lexically() {
        assert_eq!(normalize("/w", "a/./b/../c").as_deref(), Some("/w/a/c"));
        assert_eq!(normalize("/w", "../../..").as_deref(), Some("/"));
        assert_eq!(normalize("/w", "~/.ssh//id").as_deref(), Some("~/.ssh/id"));
        assert_eq!(normalize("/w", "~/../x"), None);
        assert!(under("/w/a", "/w") && under("/w", "/w/") && !under("/wx", "/w"));
    }

    #[test]
    fn globs() {
        assert!(glob("~/.ssh/**", "~/.ssh"));
        assert!(glob("~/.ssh/**", "~/.ssh/a/b"));
        assert!(glob("~/.cargo/credentials*", "~/.cargo/credentials.toml"));
        assert!(glob("/proc/*/environ", "/proc/12/environ"));
        assert!(!glob("/proc/*/environ", "/proc/12/x/environ"));
        assert_eq!(literal_base("~/.aws/**"), "~/.aws");
        assert_eq!(literal_base("/proc/*/environ"), "/proc");
        assert!(glob("~/.[s]sh/**", "~/.ssh/id"));
        assert!(glob("[!a]b", "cb") && !glob("[!a]b", "ab"));
        assert!(overlap("~/.s*/id_rsa", "~/.ssh/**"));
        assert!(overlap("~/.*", "~/.aws/**"));
        assert!(overlap("~/.npm*", "~/.npmrc"));
        assert!(!overlap("~/*", "~/.aws/**"));
        assert!(!overlap("*.rs", ".env*"));
        assert!(!overlap("~", "~/.aws/**"));
        assert!(!overlap("/proc/*/status", "/proc/*/environ"));
        assert!(overlap("/proc/*/env*", "/proc/*/environ"));
        assert!(overlap("~/Library/K*", "~/Library/Keychains/**"));
    }
}
