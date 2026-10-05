//! Public symbol and signature extractor for Rust, TypeScript/JavaScript, Python, Go (P5.m1c).

use std::path::Path;

/// Maximum number of lines to scan per file for symbols to keep extraction fast.
const MAX_SCAN_LINES: usize = 300;

/// Extracts public symbol signatures from supported source code files.
pub async fn extract_file_symbols(file_path: &Path, rel_path: &str) -> Vec<String> {
    let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let content = match tokio::fs::read_to_string(file_path).await {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    match ext {
        "rs" => extract_rust_symbols(&content, rel_path),
        "ts" | "tsx" | "js" | "jsx" => extract_ts_symbols(&content, rel_path),
        "py" => extract_python_symbols(&content, rel_path),
        "go" => extract_go_symbols(&content, rel_path),
        _ => Vec::new(),
    }
}

fn extract_rust_symbols(content: &str, rel_path: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for line in content.lines().take(MAX_SCAN_LINES) {
        let trimmed = line.trim();
        if trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("pub trait ")
            || trimmed.starts_with("pub type ")
        {
            let sig = trimmed.split('{').next().unwrap_or(trimmed).trim_end_matches(';').trim();
            symbols.push(format!("{rel_path}: {sig}"));
        }
    }
    symbols
}

fn extract_ts_symbols(content: &str, rel_path: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for line in content.lines().take(MAX_SCAN_LINES) {
        let trimmed = line.trim();
        if trimmed.starts_with("export function ")
            || trimmed.starts_with("export async function ")
            || trimmed.starts_with("export interface ")
            || trimmed.starts_with("export type ")
            || trimmed.starts_with("export class ")
            || trimmed.starts_with("export const ")
        {
            let sig = trimmed.split('{').next().unwrap_or(trimmed).trim_end_matches(';').trim();
            symbols.push(format!("{rel_path}: {sig}"));
        }
    }
    symbols
}

fn extract_python_symbols(content: &str, rel_path: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for line in content.lines().take(MAX_SCAN_LINES) {
        let trimmed = line.trim();
        if trimmed.starts_with("def ") && !trimmed.starts_with("def _") {
            let sig = trimmed.split(':').next().unwrap_or(trimmed).trim();
            symbols.push(format!("{rel_path}: {sig}"));
        } else if trimmed.starts_with("class ") && !trimmed.starts_with("class _") {
            let sig = trimmed.split(':').next().unwrap_or(trimmed).trim();
            symbols.push(format!("{rel_path}: {sig}"));
        }
    }
    symbols
}

fn extract_go_symbols(content: &str, rel_path: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for line in content.lines().take(MAX_SCAN_LINES) {
        let trimmed = line.trim();
        if trimmed.starts_with("func ") {
            // In Go, uppercase first letter after `func ` or `func (recv) ` is exported
            let after_func = trimmed.trim_start_matches("func ").trim();
            let name_part = if after_func.starts_with('(') {
                after_func.split(')').nth(1).map(str::trim).unwrap_or("")
            } else {
                after_func
            };
            if let Some(first_char) = name_part.chars().next() {
                if first_char.is_uppercase() {
                    let sig = trimmed.split('{').next().unwrap_or(trimmed).trim();
                    symbols.push(format!("{rel_path}: {sig}"));
                }
            }
        } else if trimmed.starts_with("type ") {
            let after_type = trimmed.trim_start_matches("type ").trim();
            if let Some(first_char) = after_type.chars().next() {
                if first_char.is_uppercase() {
                    let sig = trimmed.split('{').next().unwrap_or(trimmed).trim();
                    symbols.push(format!("{rel_path}: {sig}"));
                }
            }
        }
    }
    symbols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_symbols() {
        let code = r#"
            pub struct Engine {
                id: String,
            }

            pub async fn run_job(id: &str) -> Result<(), Error> {
                Ok(())
            }

            fn internal_helper() {}
        "#;
        let syms = extract_rust_symbols(code, "src/engine.rs");
        assert_eq!(syms.len(), 2);
        assert!(syms[0].contains("pub struct Engine"));
        assert!(syms[1].contains("pub async fn run_job(id: &str) -> Result<(), Error>"));
    }

    #[test]
    fn test_extract_ts_symbols() {
        let code = r#"
            export interface UserProfile {
                name: string;
            }

            export function getUser(): UserProfile {
                return { name: "Alice" };
            }

            function privateHelper() {}
        "#;
        let syms = extract_ts_symbols(code, "src/user.ts");
        assert_eq!(syms.len(), 2);
        assert!(syms[0].contains("export interface UserProfile"));
        assert!(syms[1].contains("export function getUser(): UserProfile"));
    }

    #[test]
    fn test_extract_go_symbols() {
        let code = r#"
            type Client struct {}

            func NewClient() *Client {
                return &Client{}
            }

            func (c *Client) FetchData() error {
                return nil
            }

            func internalHelper() {}
        "#;
        let syms = extract_go_symbols(code, "pkg/client.go");
        assert_eq!(syms.len(), 3);
        assert!(syms[0].contains("type Client struct"));
        assert!(syms[1].contains("func NewClient() *Client"));
        assert!(syms[2].contains("func (c *Client) FetchData() error"));
    }
}
