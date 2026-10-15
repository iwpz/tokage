//! Aider CLI adapter: `aider --message …`.
//! Sample: tests/fixtures/aider.txt (Aider 0.72.0).
//!
//! Aider does not produce JSONL; it prints formatted text to stdout.
//! This adapter parses model lines, applied edits, message text, and the tail
//! `Tokens: ... sent, ... received. Cost: $... message` summary into structured events.

use super::{rel, ChatRequest, EventKind, FileTouch, Launch, Out, Parser};

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args: Vec<String> = vec![
        "--message".into(),
        prompt,
        "--model".into(),
        req.model.clone(),
        "--no-git".into(),
        "--no-auto-commits".into(),
        "--no-check-update".into(),
        "--no-analytics".into(),
        "--no-fancy-input".into(),
        "--no-pretty".into(),
        "--yes-always".into(),
    ];
    let role = req.effective_role();
    if role.is_read_only() {
        args.push("--dry-run".into());
    }
    if let Some(_id) = &req.resume {
        args.push("--restore-chat-history".into());
    }

    Launch {
        args,
        stdin: None,
        env_remove: vec![],
        parser: Box::new(Aider {
            cwd: req.cwd.clone(),
            model: Some(req.model.clone()),
            ..Default::default()
        }),
    }
}

#[derive(Default)]
pub struct Aider {
    cwd: String,
    model: Option<String>,
    session_id_emitted: bool,
    usage: Option<tokage_core::event::TurnUsage>,
    finished: bool,
}

impl Parser for Aider {
    fn line(&mut self, _v: &serde_json::Value, _out: &mut Out) {
        // Aider output is non-JSON; line events are handled via text_line.
    }

    fn text_line(&mut self, line: &str, out: &mut Out) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }

        // 1. Quota check
        if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::Aider), trimmed) {
            out.quota(qm.raw, qm.reset_at, qm.retrying);
        }

        // 2. Model declaration: "Model: deepseek/deepseek-chat ..."
        if let Some(rest) = trimmed.strip_prefix("Model: ") {
            let model_name = rest.split_whitespace().next().unwrap_or(rest);
            self.model = Some(model_name.to_string());
            out.emit(EventKind::Model {
                name: model_name.to_string(),
            });
            return;
        }

        // 3. Tokens & Cost line:
        // "Tokens: 1.2k sent, 420 received. Cost: $0.0120 message, $0.0120 session."
        // or "Tokens: 12,500 sent, 1,350 received."
        if trimmed.starts_with("Tokens:") {
            if let Some(u) = parse_aider_tokens_cost(trimmed) {
                self.usage = Some(u);
                return;
            }
        }

        // 4. File edit: "Applied edit to src/lib.rs"
        if let Some(rest) = trimmed.strip_prefix("Applied edit to ") {
            let path = rest.trim();
            out.file(FileTouch {
                tool_id: None,
                path: rel(&self.cwd, path),
                before: None,
                after: None,
                deleted: false,
            });
            return;
        }

        // 5. Skip banner/status lines
        if trimmed.starts_with("Aider v")
            || trimmed.starts_with("Git repo:")
            || trimmed.starts_with("Repo-map:")
            || trimmed.starts_with("Main model:")
            || trimmed.starts_with("Weak model:")
        {
            return;
        }

        // 6. Regular prose text
        out.begin_text();
        out.text(trimmed);
    }

    fn finish(&mut self, out: &mut Out) {
        if !self.session_id_emitted {
            out.emit(EventKind::Thread {
                id: "aider-session".into(),
            });
            self.session_id_emitted = true;
        }

        if !self.finished {
            self.finished = true;
            let u = self.usage.clone().unwrap_or(tokage_core::event::TurnUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                api_calls: Some(1),
                context_tokens: None,
                cost_usd: None,
                premium_requests: None, credits: None,
                source: tokage_core::UsageSource::StdoutRegex,
            });

            out.emit(EventKind::Done {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_read_tokens: u.cache_read_tokens,
                cache_write_tokens: u.cache_write_tokens,
                api_calls: u.api_calls,
                context_tokens: u.context_tokens,
                cost_usd: u.cost_usd,
                premium_requests: u.premium_requests, credits: None,
                source: u.source,
            });
        }
    }
}

/// Parses "Tokens: 1.2k sent, 420 received. Cost: $0.0120 message, $0.0120 session."
fn parse_aider_tokens_cost(line: &str) -> Option<tokage_core::event::TurnUsage> {
    let tokens_part = line.strip_prefix("Tokens:")?.trim();
    let sent_str = tokens_part.split("sent").next()?.trim();
    let after_sent = tokens_part.split("sent,").nth(1)?;
    let recv_str = after_sent.split("received").next()?.trim();

    let input_tokens = parse_token_count(sent_str);
    let output_tokens = parse_token_count(recv_str);

    let cost_usd = if let Some(cost_part) = line.split("Cost:").nth(1) {
        cost_part
            .split('$')
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<f64>().ok())
    } else {
        None
    };

    Some(tokage_core::event::TurnUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        api_calls: Some(1),
        context_tokens: Some(input_tokens + output_tokens),
        cost_usd,
        premium_requests: None, credits: None,
        source: tokage_core::UsageSource::StdoutRegex,
    })
}

/// Parses "1.2k", "12,500", "420" into numeric token count.
fn parse_token_count(s: &str) -> u64 {
    let clean = s.replace(',', "");
    let s = clean.trim();
    if let Some(n) = s.strip_suffix(['k', 'K']) {
        if let Ok(f) = n.parse::<f64>() {
            return (f * 1000.0).round() as u64;
        }
    }
    if let Some(n) = s.strip_suffix(['m', 'M']) {
        if let Ok(f) = n.parse::<f64>() {
            return (f * 1_000_000.0).round() as u64;
        }
    }
    s.parse::<u64>().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::testing::*;

    #[test]
    fn parses_tokens_and_cost() {
        let u = parse_aider_tokens_cost(
            "Tokens: 1.2k sent, 420 received. Cost: $0.0120 message, $0.0120 session.",
        )
        .unwrap();
        assert_eq!(u.input_tokens, 1200);
        assert_eq!(u.output_tokens, 420);
        assert_eq!(u.cost_usd, Some(0.012));
        assert_eq!(u.source, tokage_core::UsageSource::StdoutRegex);

        let u2 = parse_aider_tokens_cost("Tokens: 15,300 sent, 2,500 received. Cost: $0.045 message.").unwrap();
        assert_eq!(u2.input_tokens, 15300);
        assert_eq!(u2.output_tokens, 2500);
        assert_eq!(u2.cost_usd, Some(0.045));
    }

    #[test]
    fn parses_fixture() {
        let mut p = Aider {
            cwd: "/work".into(),
            ..Default::default()
        };
        let evs = replay(&mut p, "aider.txt");
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Thread { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Model { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Text { .. })), 2);
        assert!(evs.iter().any(|e| matches!(e, crate::chat::Ev::File(f) if f.path == "src/lib.rs")));
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Done { .. })), 1);

        let done = evs
            .iter()
            .find_map(|e| match e {
                crate::chat::Ev::Emit(k @ EventKind::Done { .. }) => Some(k.clone()),
                _ => None,
            })
            .expect("Done event");
        let u = done.usage().expect("usage");
        assert_eq!(u.input_tokens, 1200);
        assert_eq!(u.output_tokens, 420);
        assert_eq!(u.cost_usd, Some(0.012));
        assert_eq!(u.source, tokage_core::UsageSource::StdoutRegex);
    }
}
