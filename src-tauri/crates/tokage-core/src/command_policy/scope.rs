//! Unified write_scope matching (docs/11 §3, fix-todo F4 / D4-A).
//!
//! All orchestration / planning / classify_edit paths should go through these helpers so
//! `src/server/**`, directory forms like `src/users/`, and shell globs share one meaning.

use super::path;

/// Normalize a write_scope entry to a glob:
/// strip `./`, `\` → `/`, collapse `/`, trim trailing `/`;
/// `.` / empty → `**`; no wildcards → `<entry>/**`.
pub fn normalize_scope(entry: &str) -> String {
    // F4.a stub: identity so table tests fail until F4.b.
    entry.to_string()
}

/// Normalize a relative file path for matching (no trailing slash expansion).
pub fn normalize_path(file: &str) -> String {
    // F4.a stub.
    file.to_string()
}

/// Whether a (possibly unnormalized) scope entry covers `file`.
pub fn covers(scope: &str, file: &str, fold_case: bool) -> bool {
    let _ = (scope, file, fold_case);
    // F4.a stub: always miss.
    false
}

pub fn any_covers(scopes: &[String], file: &str, fold_case: bool) -> bool {
    scopes.iter().any(|s| covers(s, file, fold_case))
}

/// Whether two (possibly unnormalized) scope entries overlap.
pub fn overlap(a: &str, b: &str, fold_case: bool) -> bool {
    let _ = (a, b, fold_case, path::overlap);
    false
}

/// First overlapping pair across two scope lists, if any.
pub fn scopes_overlap(
    a: &[String],
    b: &[String],
    fold_case: bool,
) -> Option<(String, String)> {
    for x in a {
        for y in b {
            if overlap(x, y, fold_case) {
                return Some((x.clone(), y.clone()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_normalize_basics() {
        assert_eq!(normalize_scope(""), "**");
        assert_eq!(normalize_scope("."), "**");
        assert_eq!(normalize_scope("./src/users/"), "src/users/**");
        assert_eq!(normalize_scope("src\\server\\"), "src/server/**");
        assert_eq!(normalize_scope("src//server///"), "src/server/**");
        assert_eq!(normalize_scope("src/main.rs"), "src/main.rs/**");
        assert_eq!(normalize_scope("src/server/**"), "src/server/**");
        assert_eq!(normalize_scope("**/*.ts"), "**/*.ts");
        assert_eq!(normalize_scope(".github/**"), ".github/**");
    }

    #[test]
    fn scope_covers_table() {
        let cases: &[(&str, &str, bool)] = &[
            ("src/server/**", "src/server/routes.rs", true),
            ("src/users/", "src/users/a/b.rs", true),
            ("src/main.rs", "src/main.rs", true),
            ("src/main.rs", "src/other.rs", false),
            ("**/*.ts", "a/b/c.ts", true),
            ("*.md", "README.md", true),
            ("*.md", "docs/README.md", false),
            (".github/**", ".github/workflows/ci.yml", true),
            (".", "anything/here.rs", true),
            ("", "src/x.rs", true),
            (r"src\server\**", "src/server/routes.rs", true),
        ];
        for &(scope, file, want) in cases {
            assert_eq!(
                covers(scope, file, false),
                want,
                "covers({scope:?}, {file:?}) should be {want}"
            );
        }
    }

    #[test]
    fn scope_covers_fold_case() {
        assert!(
            covers("Src/Server/**", "src/server/routes.rs", true),
            "fold_case should cover"
        );
        assert!(
            !covers("Src/Server/**", "src/server/routes.rs", false),
            "case-sensitive miss"
        );
    }

    #[test]
    fn scope_overlap_table() {
        let cases: &[(&str, &str, bool)] = &[
            ("src/a/**", "src/ab/**", false),
            ("src/**", "src/server/**", true),
            ("**/*.ts", "src/x.ts", true),
            ("src/server/**", "src/server/routes/**", true),
            ("src/users/", "src/users/a/**", true),
            (r"src\a\**", "src/a/b/**", true),
        ];
        for &(a, b, want) in cases {
            assert_eq!(
                overlap(a, b, false),
                want,
                "overlap({a:?}, {b:?}) should be {want}"
            );
            assert_eq!(
                overlap(b, a, false),
                want,
                "overlap({b:?}, {a:?}) should be {want}"
            );
        }
    }

    #[test]
    fn scope_scopes_overlap_returns_pair() {
        let a = vec!["src/**".into()];
        let b = vec!["src/server/**".into(), "docs/**".into()];
        let hit = scopes_overlap(&a, &b, false);
        assert!(hit.is_some(), "src/** should overlap src/server/**");
        let (x, y) = hit.unwrap();
        assert!(
            (x == "src/**" && y == "src/server/**") || (x == "src/server/**" && y == "src/**")
        );

        let c = vec!["src/a/**".into()];
        let d = vec!["src/ab/**".into()];
        assert!(scopes_overlap(&c, &d, false).is_none());
    }
}
