//! Local zero-token triage engine (docs/02 §2, P5.m4).
//!
//! Triage inspects prompt text and local project context (RepoDigest) with zero tokens
//! to decide whether a user request should be planned (PLAN) or directly dispatched (DIRECT).

use std::collections::{HashMap, HashSet};
use regex::Regex;
use tokage_core::planning::{RepoDigest, TriageResult, TriageRoute, TriageRuleMatch};

/// Domain keyword dictionary mapping domain tags to Chinese and English terms.
pub fn default_domain_keywords() -> HashMap<&'static str, Vec<&'static str>> {
    let mut map = HashMap::new();
    map.insert(
        "frontend-ui",
        vec![
            "组件", "样式", "页面", "界面", "交互", "前端", "css", "tailwind", "react", "vue",
            "svelte", "component", "ui", "layout", "a11y", "accessibility", "html", "dom",
        ],
    );
    map.insert(
        "backend-api",
        vec![
            "接口", "api", "服务端", "后端", "鉴权", "endpoint", "handler", "route", "auth",
            "middleware", "grpc", "rest", "controller", "rpc", "jwt",
        ],
    );
    map.insert(
        "data-model",
        vec![
            "数据库", "表结构", "迁移", "查询", "sql", "schema", "migration", "index", "orm",
            "sqlite", "postgres", "mysql", "database", "redis", "dao",
        ],
    );
    map.insert(
        "algorithm",
        vec![
            "算法", "性能", "复杂度", "优化", "algorithm", "performance", "complexity",
            "optimize", "concurrency", "cache", "lru",
        ],
    );
    map.insert(
        "integration",
        vec![
            "联调", "合并冲突", "集成", "integration", "merge conflict", "wire up", "end-to-end",
        ],
    );
    map.insert(
        "refactor",
        vec![
            "重构", "迁移", "重命名", "拆分", "解耦", "refactor", "rename", "extract",
            "restructure", "upgrade",
        ],
    );
    map.insert(
        "infra",
        vec![
            "构建", "部署", "ci", "docker", "k8s", "pipeline", "build", "deploy",
            "github actions", "terraform", "nginx", "k8s", "container",
        ],
    );
    map.insert(
        "testing",
        vec![
            "测试", "单测", "用例", "覆盖率", "test", "spec", "coverage", "e2e", "mock",
            "integration test", "unit test",
        ],
    );
    map.insert(
        "docs",
        vec![
            "文档", "注释", "readme", "docs", "documentation", "changelog", "comment",
        ],
    );
    map.insert(
        "review",
        vec!["评审", "审查", "review", "audit", "检查一下", "代码审查"],
    );
    map
}

/// Configuration options for local triage.
#[derive(Clone, Debug)]
pub struct TriageOptions {
    pub min_domain_count: usize,
    pub multi_domain_weight: f64,
    pub many_files_min: usize,
    pub many_files_weight: f64,
    pub feature_weight: f64,
    pub single_issue_weight: f64,
    pub pinpoint_location_weight: f64,
    pub short_message_max_chars: usize,
    pub short_message_weight: f64,
    pub confidence_threshold: f64,
}

impl Default for TriageOptions {
    fn default() -> Self {
        Self {
            min_domain_count: 2,
            multi_domain_weight: 3.0,
            many_files_min: 4,
            many_files_weight: 2.0,
            feature_weight: 2.0,
            single_issue_weight: 3.0,
            pinpoint_location_weight: 2.0,
            short_message_max_chars: 80,
            short_message_weight: 1.0,
            confidence_threshold: 0.15,
        }
    }
}

/// Local zero-token triage engine evaluating prompt and project context.
pub struct TriageEngine {
    options: TriageOptions,
    domain_keywords: HashMap<&'static str, Vec<&'static str>>,
    feature_regex_en: Regex,
    single_issue_regex_en: Regex,
    pinpoint_regex: Regex,
    file_path_regex: Regex,
}

impl Default for TriageEngine {
    fn default() -> Self {
        Self::new(TriageOptions::default())
    }
}

impl TriageEngine {
    pub fn new(options: TriageOptions) -> Self {
        let feature_regex_en = Regex::new(
            r"(?i)\b(implement|build|scaffold|create an? (?:app|service|module|feature|system|pipeline)|migrate (?:to|from)|refactor (?:the )?(?:whole|entire)|from scratch|end[- ]to[- ]end|architecture)\b"
        ).expect("Valid feature regex");

        let single_issue_regex_en = Regex::new(
            r"(?i)\b(this error|explain|what does|why does|fix (?:this|the) (?:typo|bug|line|function|test)|rename|stack ?trace|quick fix|what is|how to use|syntax error)\b"
        ).expect("Valid single issue regex");

        let pinpoint_regex = Regex::new(
            r"[\w./\\-]+\.[a-zA-Z]{1,6}:\d+|(?i)(?:line|at line|第)\s*\d+\s*(?:行)?"
        ).expect("Valid pinpoint regex");

        let file_path_regex = Regex::new(
            r"(?:[\w-]+\/)+[\w-]+\.[a-zA-Z0-9]{1,8}|[\w-]+\.(?:rs|ts|tsx|js|jsx|py|go|java|c|cpp|h|hpp|vue|svelte|css|scss|html|json|yaml|yml|toml|sql|md|sh)"
        ).expect("Valid file path regex");

        Self {
            options,
            domain_keywords: default_domain_keywords(),
            feature_regex_en,
            single_issue_regex_en,
            pinpoint_regex,
            file_path_regex,
        }
    }

    /// Evaluates prompt and returns a structured TriageResult with zero token cost.
    pub fn triage(&self, prompt: &str, digest: Option<&RepoDigest>) -> TriageResult {
        let text = prompt.trim();
        let lower = text.to_lowercase();
        let mut matches = Vec::new();
        let mut score_plan = 0.0;
        let mut score_direct = 0.0;

        // 1. Domain detection
        let mut detected_domains = Vec::new();
        for (domain, keywords) in &self.domain_keywords {
            let matched = keywords.iter().any(|kw| {
                if kw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    // English word boundary match
                    let pattern = format!(r"(?i)\b{}\b", regex::escape(kw));
                    Regex::new(&pattern).map(|r| r.is_match(&lower)).unwrap_or(false)
                } else {
                    lower.contains(kw)
                }
            });
            if matched {
                detected_domains.push(domain.to_string());
            }
        }
        detected_domains.sort();

        // Signal 1: Multiple domains mentioned (>= 2)
        if detected_domains.len() >= self.options.min_domain_count {
            let weight = self.options.multi_domain_weight;
            score_plan += weight;
            matches.push(TriageRuleMatch {
                rule_id: "multi-domain".to_string(),
                toward: TriageRoute::Plan,
                weight,
                reason: format!(
                    "涉及到多个领域 ({} 个: {})",
                    detected_domains.len(),
                    detected_domains.join(", ")
                ),
            });
        }

        // 2. Feature / Architecture / Module level keywords (Signal 2)
        let feature_keywords_zh = [
            "实现", "开发", "搭建", "新增功能", "重构整个", "设计一个", "从零", "迁移到",
            "整个项目", "模块", "系统", "全套", "流程", "架构", "设计模式",
        ];
        let has_feature_zh = feature_keywords_zh.iter().any(|kw| text.contains(kw));
        let has_feature_en = self.feature_regex_en.is_match(text);

        if has_feature_zh || has_feature_en {
            let weight = self.options.feature_weight;
            score_plan += weight;
            matches.push(TriageRuleMatch {
                rule_id: "feature-description".to_string(),
                toward: TriageRoute::Plan,
                weight,
                reason: "包含新功能/模块/架构级描述信号".to_string(),
            });
        }

        // 3. Estimated files involved (Signal 3: > 3 files)
        let mut estimated_files = HashSet::new();
        for cap in self.file_path_regex.find_iter(text) {
            let path_str = cap.as_str().trim_matches('`').to_string();
            if !path_str.is_empty() {
                estimated_files.insert(path_str);
            }
        }

        // If repo digest is present, scan for occurrences of known files or symbols
        if let Some(d) = digest {
            for hotspot in &d.hotspots {
                let file_name = hotspot.path.rsplit('/').next().unwrap_or(&hotspot.path);
                if text.contains(file_name) || text.contains(&hotspot.path) {
                    estimated_files.insert(hotspot.path.clone());
                }
            }
            for manifest in &d.manifests {
                let manifest_name = manifest.path.rsplit('/').next().unwrap_or(&manifest.path);
                if text.contains(manifest_name) || text.contains(&manifest.path) {
                    estimated_files.insert(manifest.path.clone());
                }
            }
        }

        let mut files_vec: Vec<String> = estimated_files.into_iter().collect();
        files_vec.sort();

        if files_vec.len() >= self.options.many_files_min {
            let weight = self.options.many_files_weight;
            score_plan += weight;
            matches.push(TriageRuleMatch {
                rule_id: "many-files".to_string(),
                toward: TriageRoute::Plan,
                weight,
                reason: format!(
                    "预计涉及较多文件 ({} 个: {})",
                    files_vec.len(),
                    files_vec.iter().take(4).cloned().collect::<Vec<_>>().join(", ")
                ),
            });
        }

        // 4. Single issue / Question / Pointed fix (Signal 4)
        let single_issue_keywords_zh = [
            "这个报错", "报错", "解释", "改一下", "这个函数", "这一行", "为什么",
            "是什么意思", "修一下", "拼写", "看下这个", "怎么用", "怎么写", "单测报错",
            "编译报错", "语法错误", "这行代码",
        ];
        let has_single_issue_zh = single_issue_keywords_zh.iter().any(|kw| text.contains(kw));
        let has_single_issue_en = self.single_issue_regex_en.is_match(text);

        if has_single_issue_zh || has_single_issue_en {
            let weight = self.options.single_issue_weight;
            score_direct += weight;
            matches.push(TriageRuleMatch {
                rule_id: "single-issue".to_string(),
                toward: TriageRoute::Direct,
                weight,
                reason: "包含单一问题/具体疑问/报错修复信号".to_string(),
            });
        }

        // 5. Pinpoint location (Signal 5)
        if self.pinpoint_regex.is_match(text) {
            let weight = self.options.pinpoint_location_weight;
            score_direct += weight;
            matches.push(TriageRuleMatch {
                rule_id: "pinpoint-location".to_string(),
                toward: TriageRoute::Direct,
                weight,
                reason: "指向具体代码位置或行号".to_string(),
            });
        }

        // 6. Short message (Signal 6: <= 80 chars)
        if text.chars().count() <= self.options.short_message_max_chars && !text.is_empty() {
            let weight = self.options.short_message_weight;
            score_direct += weight;
            matches.push(TriageRuleMatch {
                rule_id: "short-message".to_string(),
                toward: TriageRoute::Direct,
                weight,
                reason: format!("消息简短 (<= {} 字符)", self.options.short_message_max_chars),
            });
        }

        // 7. Route and Confidence resolution
        let (route, confidence, reason) = self.resolve_route(score_plan, score_direct, &matches);

        TriageResult {
            route,
            confidence,
            detected_domains,
            matched_rules: matches,
            estimated_files: files_vec,
            score_plan,
            score_direct,
            reason,
        }
    }

    fn resolve_route(
        &self,
        score_plan: f64,
        score_direct: f64,
        matches: &[TriageRuleMatch],
    ) -> (TriageRoute, f64, String) {
        let total = score_plan + score_direct;

        // If no signals fired at all, or signals are tied
        if total == 0.0 {
            return (
                TriageRoute::Plan,
                0.2,
                "未匹配到明确特征，置信度不足，默认采用轻量规划形式 (PLAN)".to_string(),
            );
        }

        let margin = (score_plan - score_direct).abs();
        let raw_confidence = margin / total;

        if score_plan > score_direct {
            // Plan wins
            let reasons: Vec<&str> = matches
                .iter()
                .filter(|m| m.toward == TriageRoute::Plan)
                .map(|m| m.reason.as_str())
                .collect();
            let summary = format!(
                "判定为规划任务 (PLAN，得分 {:.1} vs {:.1})：{}",
                score_plan,
                score_direct,
                reasons.join("；")
            );
            (TriageRoute::Plan, raw_confidence.max(0.3), summary)
        } else if score_direct > score_plan {
            // Direct requires at least one substantive direct signal (single issue or pinpoint location)
            // As per docs/02 §2: "消息很短且指向具体位置", pure shortness without issue/location is ambiguous.
            let has_substantive_direct = matches
                .iter()
                .any(|m| m.rule_id == "single-issue" || m.rule_id == "pinpoint-location");

            if has_substantive_direct && margin >= 1.0 && raw_confidence >= self.options.confidence_threshold {
                let reasons: Vec<&str> = matches
                    .iter()
                    .filter(|m| m.toward == TriageRoute::Direct)
                    .map(|m| m.reason.as_str())
                    .collect();
                let summary = format!(
                    "判定为直接分发 (DIRECT，得分 {:.1} vs {:.1})：{}",
                    score_direct,
                    score_plan,
                    reasons.join("；")
                );
                (TriageRoute::Direct, raw_confidence.max(0.3), summary)
            } else {
                // Marginal difference or lacking substantive direct signal: confidence is insufficient, default to Plan
                (
                    TriageRoute::Plan,
                    0.25,
                    format!(
                        "缺少明确的单一问题或具体位置特征 (得分 {:.1} vs {:.1})，置信度不足，保守采用轻量规划 (PLAN)",
                        score_direct, score_plan
                    ),
                )
            }
        } else {
            // score_plan == score_direct (tied) -> default to Plan
            (
                TriageRoute::Plan,
                0.2,
                format!(
                    "规划与直接分发信号平衡 (得分均为 {:.1})，置信度不足，默认采用轻量规划 (PLAN)",
                    score_plan
                ),
            )
        }
    }
}

/// Convenience function to triage a prompt with default options and optional RepoDigest.
pub fn triage(prompt: &str, digest: Option<&RepoDigest>) -> TriageResult {
    TriageEngine::default().triage(prompt, digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_domain_routes_to_plan() {
        let prompt = "需要开发用户设置页面组件，同时提供后端的更新接口和数据库迁移脚本。";
        let res = triage(prompt, None);
        assert_eq!(res.route, TriageRoute::Plan);
        assert!(res.detected_domains.contains(&"frontend-ui".to_string()));
        assert!(res.detected_domains.contains(&"backend-api".to_string()));
        assert!(res.detected_domains.contains(&"data-model".to_string()));
        assert!(res.score_plan >= 3.0);
    }

    #[test]
    fn test_feature_scaffold_routes_to_plan() {
        let prompt = "从零搭建一个基于 Rust 的分布式任务队列系统，包含重试和持久化。";
        let res = triage(prompt, None);
        assert_eq!(res.route, TriageRoute::Plan);
        assert!(res.matched_rules.iter().any(|r| r.rule_id == "feature-description"));
    }

    #[test]
    fn test_feature_en_routes_to_plan() {
        let prompt = "Implement an end-to-end authentication flow with refresh tokens and oauth2.";
        let res = triage(prompt, None);
        assert_eq!(res.route, TriageRoute::Plan);
    }

    #[test]
    fn test_single_error_routes_to_direct() {
        let prompt = "这个报错怎么解决？Caused by: std::io::Error: NotFound";
        let res = triage(prompt, None);
        assert_eq!(res.route, TriageRoute::Direct);
        assert!(res.matched_rules.iter().any(|r| r.rule_id == "single-issue"));
    }

    #[test]
    fn test_pinpoint_location_routes_to_direct() {
        let prompt = "src/auth/jwt.rs:42 这一行代码报空指针异常，帮忙看一下。";
        let res = triage(prompt, None);
        assert_eq!(res.route, TriageRoute::Direct);
        assert!(res.matched_rules.iter().any(|r| r.rule_id == "pinpoint-location"));
    }

    #[test]
    fn test_explain_concept_routes_to_direct() {
        let prompt = "Explain what does this function do and why does it use atomic swap?";
        let res = triage(prompt, None);
        assert_eq!(res.route, TriageRoute::Direct);
    }

    #[test]
    fn test_short_rename_routes_to_direct() {
        let prompt = "改一下这个函数名字为 getUserById";
        let res = triage(prompt, None);
        assert_eq!(res.route, TriageRoute::Direct);
    }

    #[test]
    fn test_many_files_routes_to_plan() {
        let prompt = "请同步更新 a.rs, b.rs, c.rs, d.rs 以及 e.rs 中的版本号与导出宏";
        let res = triage(prompt, None);
        assert_eq!(res.route, TriageRoute::Plan);
        assert!(res.estimated_files.len() >= 4);
    }

    #[test]
    fn test_ambiguous_short_defaults_to_plan_if_unclear() {
        let prompt = "随便看看";
        let _res = triage(prompt, None);
        // "随便看看" <= 80 chars gives short-message (weight 1.0), but no single-issue signal,
        // so lacks substantive direct signal, defaulting to PLAN.
        assert_eq!(_res.route, TriageRoute::Plan);

        let empty_res = triage("", None);
        assert_eq!(empty_res.route, TriageRoute::Plan);
        assert_eq!(empty_res.confidence, 0.2);
    }
}
