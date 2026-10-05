//! Output desensitization and secret redaction (docs/11 §6.4, R17).
//!
//! Replaces known credential values and standard API key patterns with `[REDACTED]`
//! before events are published to the bus, stored in SQLite, or displayed in the UI.

use std::collections::HashSet;

use regex::Regex;
use tokage_core::EventKind;

use super::child_env::{is_credential, CREDENTIAL_ENV};
use crate::env::ShellEnv;

pub static BUILTIN_KNOWLEDGE: &str = include_str!("../../../../resources/knowledge/builtin.json");

/// Desensitizes model text, tool outputs, diffs, and error messages.
#[derive(Clone, Debug)]
pub struct Redactor {
    exact_secrets: Vec<String>,
    regexes: Vec<Regex>,
}

impl Redactor {
    /// Builds a new Redactor with secrets from `env` and patterns from `builtin.json`.
    pub fn new(env: &ShellEnv) -> Self {
        let mut exact_secrets = HashSet::new();

        // 1. Gather all values from env matching credential names or secret suffixes
        for (k, v) in &env.vars {
            let val = v.trim();
            if val.len() >= 6 && (is_credential(k) || CREDENTIAL_ENV.contains(&k.to_ascii_uppercase().as_str())) {
                exact_secrets.insert(val.to_string());
            }
        }

        // Sort exact secrets by descending length to prevent partial prefix replacements
        let mut sorted_secrets: Vec<String> = exact_secrets.into_iter().collect();
        sorted_secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));

        // 2. Load and compile regex patterns from builtin knowledge pack
        let patterns = builtin_redaction_patterns();
        let mut regexes = Vec::with_capacity(patterns.len());
        for p in patterns {
            if let Ok(re) = Regex::new(&p) {
                regexes.push(re);
            }
        }

        Self {
            exact_secrets: sorted_secrets,
            regexes,
        }
    }

    /// Checks if a string contains any known secret value or matches any key pattern.
    pub fn contains_secret(&self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        for secret in &self.exact_secrets {
            if text.contains(secret) {
                return true;
            }
        }
        for re in &self.regexes {
            if re.is_match(text) {
                return true;
            }
        }
        false
    }

    /// Replaces all known secrets and secret patterns with `[REDACTED]`.
    pub fn redact_text(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }

        let mut out = text.to_string();

        // 1. Exact match replacements
        for secret in &self.exact_secrets {
            if out.contains(secret) {
                out = out.replace(secret, "[REDACTED]");
            }
        }

        // 2. Regex pattern replacements
        for re in &self.regexes {
            if re.is_match(&out) {
                out = re.replace_all(&out, "[REDACTED]").into_owned();
            }
        }

        out
    }

    /// Redacts all textual and diff payload fields in an EventKind.
    pub fn redact_event(&self, event: EventKind) -> EventKind {
        match event {
            EventKind::Text { text } => EventKind::Text {
                text: self.redact_text(&text),
            },
            EventKind::Reasoning { text } => EventKind::Reasoning {
                text: self.redact_text(&text),
            },
            EventKind::ToolStart {
                id,
                tool,
                title,
                detail,
            } => EventKind::ToolStart {
                id,
                tool,
                title: self.redact_text(&title),
                detail: detail.map(|d| self.redact_text(&d)),
            },
            EventKind::ToolEnd { id, ok, output } => EventKind::ToolEnd {
                id,
                ok,
                output: output.map(|o| self.redact_text(&o)),
            },
            EventKind::FileChange {
                tool_id,
                path,
                abs_path,
                preview_path,
                change,
                diff,
                additions,
                deletions,
            } => EventKind::FileChange {
                tool_id,
                path,
                abs_path,
                preview_path,
                change,
                diff: diff.map(|d| self.redact_text(&d)),
                additions,
                deletions,
            },
            EventKind::Error { message } => EventKind::Error {
                message: self.redact_text(&message),
            },
            EventKind::ApprovalRequest {
                id,
                tool_id,
                command,
                level,
                reason,
                recoverable,
            } => EventKind::ApprovalRequest {
                id,
                tool_id,
                command: self.redact_text(&command),
                level,
                reason: self.redact_text(&reason),
                recoverable,
            },
            other => other,
        }
    }
}

/// Extracts `redaction_patterns` from `builtin.json`.
fn builtin_redaction_patterns() -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(BUILTIN_KNOWLEDGE).expect("valid builtin.json");
    v["redaction_patterns"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn test_env(pairs: &[(&str, &str)]) -> ShellEnv {
        let mut vars = HashMap::new();
        for (k, v) in pairs {
            vars.insert(k.to_string(), v.to_string());
        }
        ShellEnv { vars }
    }

    #[test]
    fn redacts_exact_known_credentials() {
        let env = test_env(&[
            ("ANTHROPIC_API_KEY", "sk-ant-mysecretkey1234567890"),
            ("CUSTOM_SERVICE_TOKEN", "token_custom_secret_val"),
        ]);
        let r = Redactor::new(&env);

        let input = "Calling API with sk-ant-mysecretkey1234567890 and token_custom_secret_val done";
        let output = r.redact_text(input);
        assert_eq!(output, "Calling API with [REDACTED] and [REDACTED] done");
        assert!(r.contains_secret(input));
    }

    #[test]
    fn redacts_standard_key_formats_even_if_not_in_env() {
        let env = test_env(&[]);
        let r = Redactor::new(&env);

        let input = "Here is an Anthropic key: sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890\n\
                     And GitHub PAT: ghp_123456789012345678901234567890123456\n\
                     AWS key: AKIAIOSFODNN7EXAMPLE";
        let output = r.redact_text(input);
        assert!(!output.contains("sk-ant-api03"));
        assert!(!output.contains("ghp_123456"));
        assert!(!output.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_events_comprehensively() {
        let env = test_env(&[("OPENAI_API_KEY", "sk-proj-supersecretkey123456789")]);
        let r = Redactor::new(&env);

        let ev = EventKind::Text {
            text: "My key is sk-proj-supersecretkey123456789".into(),
        };
        let redacted = r.redact_event(ev);
        if let EventKind::Text { text } = redacted {
            assert_eq!(text, "My key is [REDACTED]");
        } else {
            panic!("Expected Text event");
        }
    }
}
