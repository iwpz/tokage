//! OutputFilter pipeline (docs/09 §1.3, R1, P3.m2).
//!
//! Enforces output silencing and data cleanliness before logs, events, or plain text
//! buffers are written or published:
//! 1. UTF-8 decode (lossy replacement of invalid byte sequences)
//! 2. CR folding (split by `\n`; inside a line, only content after the last `\r` is preserved)
//! 3. ANSI stripping (removes CSI/OSC sequences and terminal formatting)
//! 4. Noise filter (drops spinner animations and progress bar updates)
//! 5. Duplicate folding (collapses consecutive identical lines into `  (×N)`)
//! 6. Truncation (limits single large chunks > 64KB to head 16KB + tail 16KB)
//! 7. Secret redaction (integrates with `Redactor` to replace credentials with `[REDACTED]`)

use std::collections::HashSet;

use regex::Regex;
use tokage_core::EventKind;

use super::redact::Redactor;
use crate::env::ShellEnv;

pub const CHUNK_MAX: usize = 64 * 1024;
pub const TRUNCATE_HEAD: usize = 16 * 1024;
pub const TRUNCATE_TAIL: usize = 16 * 1024;

/// Output filtering pipeline for CLI output streams and events.
pub struct OutputFilter {
    redactor: Redactor,
    ansi_re: Regex,
    progress_re: Regex,
    spinner_chars: HashSet<char>,
    dedup_line: Option<String>,
    dedup_count: usize,
}

impl OutputFilter {
    pub fn new(env: &ShellEnv) -> Self {
        let redactor = Redactor::new(env);

        // Regex matching ANSI CSI, OSC, and basic escape sequences
        let ansi_re = Regex::new(
            r"(?x)
            \x1b (?:
                \[ [0-?]* [ -/]* [@-~]   # CSI sequence
              | \] [^\x07\x1b]* (?:\x07|\x1b\\) # OSC sequence
              | [@-Z\\-_]              # 2-character escape
              | \( [B0-2]              # VT100 character set
            )",
        )
        .expect("valid ansi regex");

        // Regex matching progress bar updates (e.g. `[=====>    ] 45%`, `12MB / 50MB [-->] 24%`)
        let progress_re = Regex::new(
            r"(?x)
            ^\s* (?:
                \[? [=\#>\-\s\.]{3,} \]? \s* \d{1,3}%
              | \d{1,3}% \s* \[? [=\#>\-\s\.]* \]?
              | (?: \d+(?:\.\d+)?\s*(?:[KMG]?B)? \s*/\s* \d+(?:\.\d+)?\s*(?:[KMG]?B)? | \d+/\d+ )
                \s+ \[? [=\#>\-\s\.]* \]? \s* \d{1,3}%
            ) \s*$
            ",
        )
        .expect("valid progress regex");

        let mut spinner_chars = HashSet::new();
        for ch in ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏', '◐', '◓', '◑', '◒', '←', '↖', '↑', '↗', '→', '↘', '↓', '↙', '—', '\\', '|', '/'] {
            spinner_chars.insert(ch);
        }

        Self {
            redactor,
            ansi_re,
            progress_re,
            spinner_chars,
            dedup_line: None,
            dedup_count: 0,
        }
    }

    /// Access the underlying redactor.
    pub fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    /// Decodes raw bytes to a UTF-8 string, replacing invalid sequences with U+FFFD.
    pub fn decode_lossy(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    /// Strips ANSI escape sequences from `s`.
    pub fn strip_ansi(&self, s: &str) -> String {
        if !s.contains('\x1b') {
            return s.to_string();
        }
        self.ansi_re.replace_all(s, "").into_owned()
    }

    /// Folds carriage returns (`\r`). If lines contain `\r`, keeps only the latest
    /// segment written after the last carriage return.
    pub fn fold_cr(s: &str) -> String {
        if !s.contains('\r') {
            return s.to_string();
        }

        let mut lines = Vec::new();
        for line in s.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if let Some(last) = line.rsplit('\r').next() {
                lines.push(last);
            } else {
                lines.push(line);
            }
        }
        lines.join("\n")
    }

    /// Checks if a single line is non-informative noise (spinner animation or progress bar).
    pub fn is_noise(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }

        // Single or paired spinner character
        if trimmed.chars().count() <= 2 && trimmed.chars().all(|c| self.spinner_chars.contains(&c)) {
            return true;
        }

        // Progress bar percentage pattern
        if self.progress_re.is_match(trimmed) {
            return true;
        }

        false
    }

    /// Truncates a large string chunk (> 64KB) down to head 16KB + tail 16KB.
    pub fn truncate_64k(s: &str) -> String {
        if s.len() <= CHUNK_MAX {
            return s.to_string();
        }

        let omitted = s.len() - (TRUNCATE_HEAD + TRUNCATE_TAIL);

        // Find safe character boundary near TRUNCATE_HEAD
        let mut head_end = TRUNCATE_HEAD;
        while head_end < s.len() && !s.is_char_boundary(head_end) {
            head_end += 1;
        }

        // Find safe character boundary near s.len() - TRUNCATE_TAIL
        let mut tail_start = s.len().saturating_sub(TRUNCATE_TAIL);
        while tail_start < s.len() && !s.is_char_boundary(tail_start) {
            tail_start += 1;
        }

        format!(
            "{}\n… [truncated {} bytes] …\n{}",
            &s[..head_end],
            omitted,
            &s[tail_start..]
        )
    }

    /// Pushes one line into the line-filter pipeline:
    /// - CR fold
    /// - ANSI strip
    /// - Noise filter
    /// - Secret redaction
    /// - Consecutive duplicate folding
    ///
    /// Returns zero or more filtered lines to emit.
    pub fn push_line(&mut self, raw_line: &str) -> Vec<String> {
        let cr_folded = Self::fold_cr(raw_line);
        let ansi_stripped = self.strip_ansi(&cr_folded);

        if self.is_noise(&ansi_stripped) {
            return vec![];
        }

        let truncated = Self::truncate_64k(&ansi_stripped);
        let redacted = self.redactor.redact_text(&truncated);

        let mut out = Vec::new();

        if let Some(ref prev) = self.dedup_line {
            if prev == &redacted {
                self.dedup_count += 1;
                return out;
            } else {
                // Line changed: flush duplicate count if repeated
                if self.dedup_count > 1 {
                    out.push(format!("  (×{})", self.dedup_count));
                }
            }
        }

        self.dedup_line = Some(redacted.clone());
        self.dedup_count = 1;
        out.push(redacted);
        out
    }

    /// Flushes any pending duplicate lines at the end of output stream.
    pub fn flush(&mut self) -> Option<String> {
        if self.dedup_count > 1 {
            let res = format!("  (×{})", self.dedup_count);
            self.dedup_count = 0;
            self.dedup_line = None;
            Some(res)
        } else {
            self.dedup_count = 0;
            self.dedup_line = None;
            None
        }
    }

    /// Redacts and filters an entire event (truncates large outputs and redacts secrets).
    pub fn filter_event(&self, event: EventKind) -> EventKind {
        let redacted = self.redactor.redact_event(event);
        match redacted {
            EventKind::Text { text } => EventKind::Text {
                text: Self::truncate_64k(&text),
            },
            EventKind::Reasoning { text } => EventKind::Reasoning {
                text: Self::truncate_64k(&text),
            },
            EventKind::ToolEnd { id, ok, output } => EventKind::ToolEnd {
                id,
                ok,
                output: output.map(|o| Self::truncate_64k(&o)),
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
                diff: diff.map(|d| Self::truncate_64k(&d)),
                additions,
                deletions,
            },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_filter() -> OutputFilter {
        let env = ShellEnv::default();
        OutputFilter::new(&env)
    }

    #[test]
    fn cr_folding_keeps_last_segment() {
        assert_eq!(OutputFilter::fold_cr("Loading 10%\rLoading 50%\rDone!"), "Done!");
        assert_eq!(OutputFilter::fold_cr("Line 1\r\nLine 2"), "Line 1\nLine 2");
    }

    #[test]
    fn ansi_stripping_removes_codes() {
        let f = test_filter();
        let s = "\x1b[31mRed\x1b[0m and \x1b[1;32mGreen\x1b[0m";
        assert_eq!(f.strip_ansi(s), "Red and Green");
    }

    #[test]
    fn noise_filtering_catches_spinners_and_progress() {
        let f = test_filter();
        assert!(f.is_noise("⠋"));
        assert!(f.is_noise("⠹"));
        assert!(f.is_noise("[=========>         ] 50%"));
        assert!(f.is_noise("12MB / 100MB [===>] 12%"));
        assert!(!f.is_noise("cargo build --release completed in 2.3s"));
        assert!(!f.is_noise("fn main() { println!(\"100%\"); }"));
    }

    #[test]
    fn duplicate_folding_compresses_repeating_lines() {
        let mut f = test_filter();
        let mut out = Vec::new();
        for _ in 0..10 {
            out.extend(f.push_line("repetitive line"));
        }
        if let Some(summary) = f.flush() {
            out.push(summary);
        }

        assert_eq!(out.len(), 2);
        assert_eq!(out[0], "repetitive line");
        assert_eq!(out[1], "  (×10)");
    }

    #[test]
    fn truncation_bounds_large_chunks() {
        let huge = "a".repeat(100_000);
        let truncated = OutputFilter::truncate_64k(&huge);
        assert!(truncated.len() < huge.len());
        assert!(truncated.contains("truncated 67232 bytes"));
        assert_eq!(&truncated[..10], "aaaaaaaaaa");
    }

    #[test]
    fn desensitization_merged_in_pipeline() {
        let mut env = ShellEnv::default();
        env.vars.insert("OPENAI_API_KEY".into(), "sk-proj-12345678901234567890".into());
        let mut f = OutputFilter::new(&env);

        let lines = f.push_line("\x1b[32mKey is sk-proj-12345678901234567890\x1b[0m\rFinal Key: sk-proj-12345678901234567890");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "Final Key: [REDACTED]");
    }
}
