//! Markdown formatter and 8K token budget manager for RepoDigest (P5.m1d).

use tokage_core::planning::{HotspotItem, ManifestSummary, ProjectCommands};

/// Target token budget limit for RepoDigest (docs/02 §3, P5.m1d).
pub const DEFAULT_DIGEST_MAX_TOKENS: usize = 8000;

/// Fast heuristic for estimating token count (approx 4 chars per token).
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// Assembles the Markdown digest from components and enforces the token budget.
pub fn format_and_trim_digest(
    project_path: &str,
    primary_languages: &[String],
    manifests: &[ManifestSummary],
    commands: &ProjectCommands,
    conventions: &str,
    tree_summary: &str,
    symbols: &[String],
    hotspots: &[HotspotItem],
    max_tokens: usize,
) -> (String, usize) {
    let mut header_section = format!("# Project Digest: {project_path}\n\n");
    if !primary_languages.is_empty() {
        header_section.push_str(&format!("- **Primary Languages**: {}\n", primary_languages.join(", ")));
    }

    // Build commands section (Priority 1: must be retained)
    let mut commands_section = String::from("\n## Discovered Build & Test Commands\n");
    if !commands.build.is_empty() {
        commands_section.push_str(&format!("- **Build**: `{}`\n", commands.build.join("`, `")));
    }
    if !commands.test.is_empty() {
        commands_section.push_str(&format!("- **Test**: `{}`\n", commands.test.join("`, `")));
    }
    if !commands.check.is_empty() {
        commands_section.push_str(&format!("- **Check**: `{}`\n", commands.check.join("`, `")));
    }
    if !commands.lint.is_empty() {
        commands_section.push_str(&format!("- **Lint**: `{}`\n", commands.lint.join("`, `")));
    }

    // Manifests section (Priority 1)
    let mut manifests_section = String::from("\n## Project Manifests\n");
    for m in manifests {
        let name_str = m.name.as_deref().unwrap_or("-");
        let ver_str = m.version.as_deref().unwrap_or("-");
        let pm_str = m.package_manager.as_deref().unwrap_or("-");
        manifests_section.push_str(&format!(
            "- **{}** (`{}`): name `{}`, version `{}`, package manager `{}`\n",
            m.manifest_type, m.path, name_str, ver_str, pm_str
        ));
        if !m.dependencies.is_empty() {
            let sample_deps = if m.dependencies.len() > 15 {
                format!("{} ... (+{} more)", m.dependencies[..15].join(", "), m.dependencies.len() - 15)
            } else {
                m.dependencies.join(", ")
            };
            manifests_section.push_str(&format!("  - Dependencies: {sample_deps}\n"));
        }
    }

    // Conventions section (Priority 2)
    let conventions_section = if !conventions.trim().is_empty() {
        format!("\n## Project Conventions & Instructions\n{conventions}\n")
    } else {
        String::new()
    };

    // Hotspots section (Priority 3)
    let mut hotspots_section = String::from("\n## Recent Git Hotspots\n");
    if hotspots.is_empty() {
        hotspots_section.push_str("*(No git history available)*\n");
    } else {
        for h in hotspots {
            hotspots_section.push_str(&format!("- `{}` ({} touches)\n", h.path, h.commit_count));
        }
    }

    // Base content without tree and symbols
    let base = format!("{header_section}{commands_section}{manifests_section}{conventions_section}{hotspots_section}");
    let base_tokens = estimate_tokens(&base);

    let remaining_tokens = max_tokens.saturating_sub(base_tokens);

    // Tree section (Priority 4)
    let tree_section_header = "\n## Directory Tree\n```text\n";
    let tree_section_footer = "\n```\n";
    let tree_overhead = estimate_tokens(tree_section_header) + estimate_tokens(tree_section_footer);

    // Allocate roughly half of remaining tokens to tree and half to symbols
    let tree_budget = (remaining_tokens / 2).saturating_sub(tree_overhead);
    let mut tree_text = String::new();
    let mut current_tree_chars = 0;
    let max_tree_chars = tree_budget * 4;

    for line in tree_summary.lines() {
        if current_tree_chars + line.len() + 1 > max_tree_chars {
            tree_text.push_str("... (tree trimmed)\n");
            break;
        }
        tree_text.push_str(line);
        tree_text.push('\n');
        current_tree_chars += line.len() + 1;
    }
    let formatted_tree = format!("{tree_section_header}{tree_text}{tree_section_footer}");
    let tree_tokens = estimate_tokens(&formatted_tree);

    // Symbols section (Priority 5)
    let sym_budget = remaining_tokens.saturating_sub(tree_tokens);
    let sym_header = "\n## Public Symbols & Signatures\n```text\n";
    let sym_footer = "\n```\n";
    let max_sym_chars = sym_budget.saturating_sub(estimate_tokens(sym_header) + estimate_tokens(sym_footer)) * 4;

    let mut sym_text = String::new();
    let mut current_sym_chars = 0;
    for sym in symbols {
        if current_sym_chars + sym.len() + 1 > max_sym_chars {
            sym_text.push_str("... (symbols trimmed for token budget)\n");
            break;
        }
        sym_text.push_str(sym);
        sym_text.push('\n');
        current_sym_chars += sym.len() + 1;
    }

    let formatted_syms = if !sym_text.is_empty() {
        format!("{sym_header}{sym_text}{sym_footer}")
    } else {
        String::new()
    };

    let full_md = format!("{base}{formatted_tree}{formatted_syms}");
    let total_tokens = estimate_tokens(&full_md);

    (full_md, total_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_and_trim_digest_respects_token_budget() {
        let commands = ProjectCommands {
            build: vec!["cargo build".into()],
            test: vec!["cargo test".into()],
            check: vec![],
            lint: vec![],
        };

        let mut large_tree = String::new();
        for i in 0..1000 {
            large_tree.push_str(&format!("├── file_{i}.rs\n"));
        }

        let mut large_syms = Vec::new();
        for i in 0..1000 {
            large_syms.push(format!("src/file_{i}.rs: pub fn run_{i}()"));
        }

        let (md, tokens) = format_and_trim_digest(
            "/test/project",
            &["Rust".into()],
            &[],
            &commands,
            "",
            &large_tree,
            &large_syms,
            &[],
            1000, // Strict limit 1000 tokens
        );

        assert!(tokens <= 1100); // within tolerance
        assert!(md.contains("cargo build"));
        assert!(md.contains("cargo test"));
        assert!(md.contains("trimmed"));
    }
}
