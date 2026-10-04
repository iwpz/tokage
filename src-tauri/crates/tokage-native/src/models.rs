//! Collapses per-mode catalog entries ("Opus 5.5 Low", "Opus 5.5 High Fast", …) into one
//! group per underlying model. Modes are read from the model id, which CLIs keep regular
//! (`<base>[-thinking][-<effort>][-fast]`, context as `[1m]` or `-1m`); display names are only
//! cleaned up, not parsed.

use serde::Serialize;

use crate::providers::ModelInfo;

/// Reasoning efforts in ascending order. `default` marks a variant whose id names no effort.
pub const EFFORTS: &[&str] = &[
    "default", "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGroup {
    /// Base model id with mode suffixes removed.
    pub key: String,
    pub name: String,
    /// Selectable efforts in ascending order; empty when the model has a single mode.
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    /// Selectable context sizes ("default", "1m", …); empty when there is only one.
    pub contexts: Vec<String>,
    /// Some variant runs with extended thinking.
    pub thinking: bool,
    /// A faster (priority) tier exists.
    pub fast: bool,
    /// Facts dropped from the display name, shown in the picker's info tooltip:
    /// context sizes ("1m"), "no-zdr", or another catalog tag's text.
    pub tags: Vec<String>,
    /// Routing profiles for a CLI's own auto model (Copilot `--auto-tier`); empty otherwise.
    pub tiers: Vec<String>,
    pub default_tier: Option<String>,
    /// Concrete ids accepted by the CLI. Catalogs that take effort as a parameter
    /// (Codex) have one variant with `effort: None`.
    pub variants: Vec<ModelVariant>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVariant {
    pub id: String,
    pub effort: Option<String>,
    /// Context size named by the id (`opus[1m]` → "1m"); `None` is the model's default.
    pub context: Option<String>,
    pub thinking: bool,
    pub fast: bool,
}

struct Parsed<'a> {
    base: String,
    effort: Option<&'a str>,
    context: Option<String>,
    thinking: bool,
    fast: bool,
}

/// `1m`, `200k`: a context-size token.
fn is_context(t: &str) -> bool {
    let t = t.to_ascii_lowercase();
    t.len() > 1
        && (t.ends_with('k') || t.ends_with('m'))
        && t[..t.len() - 1].chars().all(|c| c.is_ascii_digit())
}

fn parse_id(id: &str) -> Parsed<'_> {
    // Claude Code spells the long-context alias `opus[1m]`.
    let (id, mut context) = match id.strip_suffix(']').and_then(|x| x.rsplit_once('[')) {
        Some((base, ctx)) if is_context(ctx) => (base, Some(ctx.to_ascii_lowercase())),
        _ => (id, None),
    };
    let mut parts: Vec<&str> = id.split('-').collect();
    let (mut effort, mut thinking, mut fast) = (None, false, false);
    while parts.len() > 1 {
        // `gpt-5.5-extra-high` spells xhigh as two segments.
        if effort.is_none() && parts.len() > 2 && parts[parts.len() - 2..] == ["extra", "high"] {
            effort = Some("xhigh");
            parts.truncate(parts.len() - 2);
            continue;
        }
        match *parts.last().unwrap() {
            "fast" if !fast => fast = true,
            "thinking" if !thinking => thinking = true,
            c if context.is_none() && is_context(c) => context = Some(c.to_ascii_lowercase()),
            e if effort.is_none() && e != "default" && EFFORTS.contains(&e) => effort = Some(e),
            _ => break,
        }
        parts.pop();
    }
    Parsed {
        base: parts.join("-"),
        effort,
        context,
        thinking,
        fast,
    }
}

/// Drops mode words ("Low", "Extra High", "Thinking", "Fast", "(High)") from a display name.
/// Also removes zero-width characters and doubled spaces some catalogs ship.
fn strip_modes(name: &str, contexts: bool) -> String {
    const WORDS: &[&str] = &[
        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra", "thinking", "fast",
    ];
    let clean: String = name
        .chars()
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200d}' | '\u{feff}'))
        .collect();
    let tokens: Vec<&str> = clean.split_whitespace().collect();
    // Context words ("1M") only go when the group offers a choice of context sizes.
    let is_mode =
        |t: &str| WORDS.contains(&t.to_ascii_lowercase().as_str()) || (contexts && is_context(t));
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        let next = tokens.get(i + 1).copied();
        if t.eq_ignore_ascii_case("extra") && next.is_some_and(|n| n.eq_ignore_ascii_case("high")) {
            i += 2;
            continue;
        }
        if t.eq_ignore_ascii_case("(extra") && next.is_some_and(|n| n.eq_ignore_ascii_case("high)"))
        {
            i += 2;
            continue;
        }
        let inner = t.strip_prefix('(').and_then(|x| x.strip_suffix(')'));
        if is_mode(t) || inner.is_some_and(is_mode) {
            i += 1;
            continue;
        }
        out.push(t);
        i += 1;
    }
    out.join(" ")
}

/// Official-style display name: drops context sizes ("1M") and parenthesised catalog tags
/// ("(NO ZDR)", "(Preview)"), which the picker shows elsewhere or not at all.
fn friendly(name: &str) -> String {
    let mut plain = String::new();
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    plain
        .split_whitespace()
        .filter(|t| !is_context(t))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What `friendly` drops, kept for the tooltip: "Claude Fable 5 1M (NO ZDR)" → ["1m", "no-zdr"].
/// Selectable context sizes count too, so a 1M option is mentioned even when the name omits it.
fn tags(name: &str, contexts: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |t: String| {
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    };
    let (mut plain, mut inner, mut depth) = (String::new(), String::new(), 0usize);
    for c in name.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let t = inner.split_whitespace().collect::<Vec<_>>().join(" ");
                    let lower = t.to_ascii_lowercase();
                    // Mode words ("(High)") are options, not facts about the model.
                    let mode = lower.split_whitespace().all(|w| {
                        EFFORTS.contains(&w) || matches!(w, "extra" | "thinking" | "fast")
                    });
                    match lower.as_str() {
                        "no zdr" | "no-zdr" => push("no-zdr".into()),
                        _ if is_context(&lower) => push(lower),
                        _ if !mode => push(t),
                        _ => {}
                    }
                    inner.clear();
                }
            }
            _ if depth > 0 => inner.push(c),
            _ => plain.push(c),
        }
    }
    for t in plain.split_whitespace().filter(|t| is_context(t)) {
        push(t.to_ascii_lowercase());
    }
    for c in contexts.iter().filter(|c| *c != "default") {
        push(c.clone());
    }
    out
}

fn effort_rank(e: &str) -> usize {
    EFFORTS.iter().position(|x| *x == e).unwrap_or(usize::MAX)
}

/// "default" first, then ascending size.
fn context_rank(c: &str) -> u64 {
    let n: u64 = c[..c.len().saturating_sub(1)].parse().unwrap_or(0);
    match c.chars().last() {
        _ if c == "default" => 0,
        Some('m') => n * 1_000_000,
        _ => n * 1_000,
    }
}

/// Groups a flat catalog, keeping first-appearance order.
pub fn group(models: &[ModelInfo]) -> Vec<ModelGroup> {
    let mut groups: Vec<(ModelGroup, &str)> = Vec::new();
    for m in models {
        let p = parse_id(&m.id);
        let variant = ModelVariant {
            id: m.id.clone(),
            effort: p.effort.map(str::to_string),
            context: p.context,
            thinking: p.thinking,
            fast: p.fast,
        };
        match groups.iter_mut().find(|(g, _)| g.key == p.base) {
            Some((g, _)) => g.variants.push(variant),
            None => groups.push((
                ModelGroup {
                    key: p.base,
                    name: String::new(),
                    efforts: vec![],
                    default_effort: None,
                    contexts: vec![],
                    thinking: false,
                    fast: false,
                    tags: vec![],
                    tiers: vec![],
                    default_tier: None,
                    variants: vec![variant],
                },
                &m.name,
            )),
        }
    }
    groups
        .into_iter()
        .map(|(g, first_name)| {
            let mut g = merge(g, first_name);
            g.tags = tags(first_name, &g.contexts);
            let name = friendly(&g.name);
            if !name.is_empty() {
                g.name = name;
            }
            g
        })
        .collect()
}

/// Folds a group's variants into its selectable modes and a mode-free name.
fn merge(mut g: ModelGroup, first_name: &str) -> ModelGroup {
    if g.variants.len() == 1 {
        // Nothing to merge: keep the catalog's own name and a single mode.
        g.name = first_name.split_whitespace().collect::<Vec<_>>().join(" ");
        g.name
            .retain(|c| !matches!(c, '\u{200b}'..='\u{200d}' | '\u{feff}'));
        g.thinking = g.variants[0].thinking;
        g.fast = g.variants[0].fast;
        return g;
    }
    let mut contexts: Vec<String> = g
        .variants
        .iter()
        .map(|v| v.context.clone().unwrap_or_else(|| "default".into()))
        .collect();
    contexts.sort_by_key(|c| context_rank(c));
    contexts.dedup();
    if contexts.len() > 1 {
        g.contexts = contexts;
    }
    let stripped = strip_modes(first_name, !g.contexts.is_empty());
    g.name = if stripped.is_empty() {
        g.key.clone()
    } else {
        stripped
    };
    g.thinking = g.variants.iter().any(|v| v.thinking);
    g.fast = g.variants.iter().any(|v| v.fast);
    let mut efforts: Vec<String> = g
        .variants
        .iter()
        .map(|v| v.effort.clone().unwrap_or_else(|| "default".into()))
        .collect();
    efforts.sort_by_key(|e| effort_rank(e));
    efforts.dedup();
    if efforts.len() > 1 {
        g.efforts = efforts;
    }
    g
}

/// Sort key: family (text before the first number), version, then whatever follows.
/// "GPT-5.4 mini" → ("gpt", [5, 4], "mini"). Separators are normalised so ids and display
/// names spelled `claude-opus-4.8` or `Claude Opus 4.8` compare alike.
fn sort_key(name: &str) -> (String, Vec<u64>, String) {
    let norm: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if matches!(c, '-' | '_' | '/') { ' ' } else { c })
        .collect();
    let norm = norm.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some(start) = norm.find(|c: char| c.is_ascii_digit()) else {
        return (norm, vec![], String::new());
    };
    let rest = &norm[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let version = rest[..end]
        .split('.')
        .filter_map(|n| n.parse().ok())
        .collect();
    (
        norm[..start].trim().to_string(),
        version,
        rest[end..].trim().to_string(),
    )
}

/// Orders a large catalog by name: families A→Z, newest version first within a family,
/// the plain model before its variants ("GPT-5.4" before "GPT-5.4 mini"). `auto` stays on top.
pub fn sort(groups: &mut [ModelGroup]) {
    groups.sort_by_cached_key(|g| {
        let (family, version, rest) = sort_key(&g.name);
        let auto = g.key.eq_ignore_ascii_case("auto");
        (
            !auto,
            family,
            std::cmp::Reverse(version),
            rest,
        )
    });
}

/// For catalogs that list efforts as model metadata instead of separate ids (Codex).
pub fn single(
    m: &ModelInfo,
    efforts: &[String],
    default_effort: Option<String>,
    fast: bool,
) -> ModelGroup {
    let mut efforts = efforts.to_vec();
    efforts.sort_by_key(|e| effort_rank(e));
    efforts.dedup();
    ModelGroup {
        key: m.id.clone(),
        name: m.name.clone(),
        efforts,
        default_effort,
        contexts: vec![],
        thinking: false,
        fast,
        tags: vec![],
        tiers: vec![],
        default_tier: None,
        variants: vec![ModelVariant {
            id: m.id.clone(),
            effort: None,
            context: None,
            thinking: false,
            fast: false,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, name: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn cursor_catalog_collapses_modes() {
        let list = [
            m("claude-opus-5-5-low", "Claude Opus 5.5 1M Low"),
            m("claude-opus-5-5-low-fast", "Claude Opus 5.5 1M Low Fast"),
            m("claude-opus-5-5-medium", "Claude Opus 5.5 1M"),
            m("claude-opus-5-5-xhigh", "Claude Opus 5.5 1M Extra High"),
            m("claude-opus-5-5-max-fast", "Claude Opus 5.5 1M Max Fast"),
            m(
                "claude-fable-5-thinking-high",
                "Claude Fable 5 1M Thinking (NO ZDR)",
            ),
            m("claude-fable-5-low", "Claude Fable 5 1M Low (NO ZDR)"),
            m("grok-4.7-low-fast", "Grok 4.7  Low Fast\u{200b}\u{200b}"),
            m("grok-4.7-high", "Grok 4.7  High"),
            m("gpt-5.2", "GPT-5.2"),
            m("auto", "Auto"),
        ];
        let g = group(&list);
        let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Claude Opus 5.5",
                "Claude Fable 5",
                "Grok 4.7",
                "GPT-5.2",
                "Auto"
            ]
        );
        assert_eq!(g[0].efforts, ["low", "medium", "xhigh", "max"]);
        assert!(g[0].fast && !g[0].thinking);
        assert_eq!(g[0].variants.len(), 5);
        assert_eq!(g[0].tags, ["1m"]);
        assert_eq!(g[1].tags, ["no-zdr", "1m"]);
        assert!(g[2].tags.is_empty());
        assert!(g[1].thinking);
        assert_eq!(g[1].efforts, ["low", "high"]);
        assert!(g[3].efforts.is_empty());
    }

    #[test]
    fn default_effort_and_parenthesised_names() {
        let list = [
            m("gpt-5.3-codex", "Codex 5.3"),
            m("gpt-5.3-codex-high", "Codex 5.3 High"),
            m("gemini-3.1-pro-high", "Gemini 3.1 Pro (High)"),
            m("gemini-3.1-pro-low", "Gemini 3.1 Pro (Low)"),
        ];
        let g = group(&list);
        assert_eq!(g[0].efforts, ["default", "high"]);
        assert_eq!(g[1].name, "Gemini 3.1 Pro");
        assert!(g[1].tags.is_empty());
        assert_eq!(g[1].efforts, ["low", "high"]);
    }

    #[test]
    fn context_aliases_merge() {
        let list = [
            m("opus", "Opus"),
            m("opus[1m]", "Opus 1M"),
            m("sonnet", "Sonnet"),
        ];
        let g = group(&list);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].name, "Opus");
        assert_eq!(g[0].contexts, ["default", "1m"]);
        assert_eq!(g[0].tags, ["1m"]);
        assert_eq!(g[0].variants[1].context.as_deref(), Some("1m"));
        assert!(g[1].contexts.is_empty());
        let p = parse_id("claude-sonnet-5-1m-high");
        assert_eq!(
            (p.base.as_str(), p.effort, p.context.as_deref()),
            ("claude-sonnet-5", Some("high"), Some("1m"))
        );
    }

    #[test]
    fn id_suffix_parsing_keeps_base() {
        let p = parse_id("claude-opus-5-thinking-high-fast");
        assert_eq!(p.base, "claude-opus-5");
        assert_eq!(p.effort, Some("high"));
        assert!(p.thinking && p.fast);
        assert_eq!(parse_id("fast").base, "fast");
        let p = parse_id("gpt-5.5-extra-high-fast");
        assert_eq!((p.base.as_str(), p.effort), ("gpt-5.5", Some("xhigh")));
        let p = parse_id("claude-4.6-opus-high-thinking");
        assert_eq!(
            (p.base.as_str(), p.effort, p.thinking),
            ("claude-4.6-opus", Some("high"), true)
        );
        assert_eq!(
            parse_id("deepseek/deepseek-chat").base,
            "deepseek/deepseek-chat"
        );
    }

    #[test]
    fn sort_by_family_then_newest_version() {
        let ids = [
            "gpt-5.4-mini", "claude-sonnet-4.6", "gpt-5.4", "auto", "gpt-6.1-sol", "gpt-6-sol",
            "claude-sonnet-5.5", "claude-opus-4.8", "claude-sonnet-5", "gemini-3.5-flash",
            "gemini-3.8-flash", "claude-opus-5.5", "kimi-k2.7-code", "kimi-k3", "gpt-10",
        ];
        let list: Vec<_> = ids.iter().map(|i| m(i, i)).collect();
        let mut g = group(&list);
        sort(&mut g);
        let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "auto", "claude-opus-5.5", "claude-opus-4.8", "claude-sonnet-5.5", "claude-sonnet-5",
                "claude-sonnet-4.6", "gemini-3.8-flash", "gemini-3.5-flash", "gpt-10", "gpt-6.1-sol",
                "gpt-6-sol", "gpt-5.4", "gpt-5.4-mini", "kimi-k3", "kimi-k2.7-code",
            ]
        );
    }
}
