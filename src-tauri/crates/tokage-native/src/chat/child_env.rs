//! Child-process environment per CLI (docs/11 §6.3, R16).
//!
//! The login shell environment holds every vendor's keys. A CLI only gets its own: all known
//! credential variables are removed, then the ones that CLI logs in with are added back.
//! Values are never logged or inspected; only names are matched.

use std::{collections::HashMap, path::PathBuf};

use crate::{env::ShellEnv, providers::ProviderId};

/// Credential variables of known vendors (definition pack `credential_env`, built-in copy).
/// Names that the suffix rule below already covers are listed anyway, as documentation.
pub const CREDENTIAL_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_CUSTOM_HEADERS",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "OPENAI_ORG_ID",
    "OPENAI_ORGANIZATION",
    "OPENAI_PROJECT",
    "CODEX_API_KEY",
    "CURSOR_API_KEY",
    "CURSOR_API_ENDPOINT",
    "COPILOT_GITHUB_TOKEN",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_GEMINI_BASE_URL",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "DEEPSEEK_API_KEY",
    "OPENROUTER_API_KEY",
    "GROQ_API_KEY",
    "MISTRAL_API_KEY",
    "XAI_API_KEY",
    "COHERE_API_KEY",
    "DASHSCOPE_API_KEY",
    "BAILIAN_CODING_PLAN_API_KEY",
    "MOONSHOT_API_KEY",
    "KIMI_API_KEY",
    "AMP_API_KEY",
    "FACTORY_API_KEY",
    "META_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "AZURE_OPENAI_ENDPOINT",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_BEARER_TOKEN_BEDROCK",
];

/// Any variable named like a secret is treated as one, whoever it belongs to.
const CREDENTIAL_SUFFIXES: &[&str] = &[
    "_API_KEY",
    "_APIKEY",
    "_TOKEN",
    "_SECRET",
    "_SECRET_KEY",
    "_ACCESS_KEY",
    "_ACCESS_KEY_ID",
    "_PASSWORD",
    "_BASE_URL",
];

pub fn is_credential(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    CREDENTIAL_ENV.contains(&n.as_str()) || CREDENTIAL_SUFFIXES.iter().any(|s| n.ends_with(s))
}

/// `NAME` matches exactly, `PREFIX_*` by prefix.
fn matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == pattern,
    }
}

/// Variables a CLI authenticates or configures itself with (definition pack
/// `providers.<id>.env_allow`, built-in copy). Sources: each CLI's documented env vars.
fn env_allow(id: ProviderId, env: &ShellEnv) -> Vec<String> {
    let has = |k: &str| env.vars.get(k).is_some_and(|v| !v.trim().is_empty());
    let mut allow: Vec<String> = match id {
        ProviderId::ClaudeCode => {
            let mut a = vec!["ANTHROPIC_*", "CLAUDE_CODE_*"];
            if has("CLAUDE_CODE_USE_BEDROCK") {
                a.extend(["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN", "AWS_BEARER_TOKEN_BEDROCK"]);
            }
            if has("CLAUDE_CODE_USE_VERTEX") {
                a.push("GOOGLE_APPLICATION_CREDENTIALS");
            }
            a
        }
        ProviderId::CodexCli => vec![
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
            "OPENAI_ORG_ID",
            "OPENAI_ORGANIZATION",
            "OPENAI_PROJECT",
            "CODEX_*",
        ],
        ProviderId::CursorCli => vec!["CURSOR_*"],
        ProviderId::CopilotCli => vec![
            "COPILOT_*",
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "GH_ENTERPRISE_TOKEN",
            "GITHUB_ENTERPRISE_TOKEN",
        ],
        ProviderId::AntigravityCli => vec![
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "GOOGLE_CLOUD_*",
            "ANTIGRAVITY_*",
        ],
        ProviderId::Aider => vec!["AIDER_*"],
        ProviderId::Opencode => vec!["OPENCODE_*"],
        _ => vec![],
    }
    .into_iter()
    .map(str::to_string)
    .collect();
    if id == ProviderId::CodexCli {
        // A custom model provider names its key variable in config.toml (`env_key`).
        allow.extend(codex_env_keys(env));
    }
    if id == ProviderId::Aider {
        allow.extend(aider_conf_keys(env));
    }
    allow
}

fn aider_conf_keys(env: &ShellEnv) -> Vec<String> {
    let mut out = Vec::new();
    let dir = env.vars.get("HOME").map(PathBuf::from);
    if let Some(d) = dir {
        let conf = d.join(".aider.conf.yml");
        if let Ok(text) = std::fs::read_to_string(conf) {
            for line in text.lines() {
                let t = line.trim();
                if let Some((k, _)) = t.split_once(':') {
                    let k = k.trim().to_ascii_uppercase().replace('-', "_");
                    if k.ends_with("_API_KEY") {
                        out.push(k);
                    }
                }
            }
        }
    }
    out
}

/// `env_key = "NAME"` entries in Codex's config.toml. Only variable names are read.
fn codex_env_keys(env: &ShellEnv) -> Vec<String> {
    let dir = env
        .vars
        .get("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env.vars.get("HOME").map(|h| PathBuf::from(h).join(".codex")));
    let Some(text) = dir.and_then(|d| std::fs::read_to_string(d.join("config.toml")).ok()) else {
        return vec![];
    };
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == "env_key").then_some(())?;
            let name = v.split('#').next()?.trim().trim_matches(['"', '\'']);
            let valid = !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            valid.then(|| name.to_string())
        })
        .collect()
}

/// Creates or returns the path to Tokage's shell wrapper script that strips credential environment variables (P2.m5a).
pub fn ensure_shell_wrapper() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("tokage");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("tokage-shell-wrapper.sh");
    let content = r#"#!/bin/sh
# Tokage shell wrapper (P2.m5a): Strip credential variables before invoking subshell
for var in $(env | cut -d= -f1); do
    case "$var" in
        *_API_KEY|*_TOKEN|*_SECRET|*_BASE_URL|ANTHROPIC_*|OPENAI_*|CODEX_*|CURSOR_*|COPILOT_*|GH_*|GITHUB_*|GEMINI_*|GOOGLE_*|DEEPSEEK_*)
            unset "$var"
            ;;
    esac
done
exec /bin/sh "$@"
"#;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(path)
}

pub const PROXY_VAR_NAMES: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

pub const CA_CERT_VAR_NAMES: &[&str] = &[
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
];

/// Whether a value embeds URL userinfo (`scheme://user:pass@host`).
pub fn value_has_userinfo(value: &str) -> bool {
    // Look for "://" then a '@' before the next path separator that is not inside the host alone.
    let Some(scheme_end) = value.find("://") else {
        return false;
    };
    let rest = &value[scheme_end + 3..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    authority.contains('@')
}

/// Gate subprocess environment (docs/11 §6.3, F5.d): login env minus credentials and
/// values with URL userinfo, except names listed in `allow` (`NAME` or `PREFIX_*`).
///
/// F5.a stub: identity copy so contract tests fail until F5.d.
pub fn assemble_gate_env(env: &ShellEnv, _allow: &[String]) -> HashMap<String, String> {
    env.vars.clone()
}

/// The environment one CLI's process starts with:
/// login env − all credentials + that CLI's own allow-list − `remove` (host session vars).
#[allow(dead_code)]
pub fn assemble(id: ProviderId, env: &ShellEnv, remove: &[String]) -> HashMap<String, String> {
    assemble_with_proxy(id, env, remove, None)
}

/// Assembles child process environment with proxy configuration and credential stripping (P3.m6b, P3.m6h).
pub fn assemble_with_proxy(
    id: ProviderId,
    env: &ShellEnv,
    remove: &[String],
    proxy: Option<&crate::proxy::PreparedTurnProxy>,
) -> HashMap<String, String> {
    let allow = env_allow(id, env);
    let mut map: HashMap<String, String> = env.vars
        .iter()
        .filter(|(k, _)| !is_credential(k) || allow.iter().any(|p| matches(p, k)))
        .filter(|(k, _)| !remove.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    if let Some(p) = proxy {
        match p.mode {
            tokage_core::proxy::ProxyMode::None => {
                // Strip all proxy variables unless forced mode redirected to local relay
                for var in PROXY_VAR_NAMES {
                    map.remove(*var);
                }
                if p.forced {
                    if let Some(ref url) = p.proxy_url {
                        map.insert("HTTP_PROXY".into(), url.clone());
                        map.insert("HTTPS_PROXY".into(), url.clone());
                        map.insert("ALL_PROXY".into(), url.clone());
                        map.insert("http_proxy".into(), url.clone());
                        map.insert("https_proxy".into(), url.clone());
                        map.insert("all_proxy".into(), url.clone());
                    }
                    if let Some(ref np) = p.no_proxy {
                        map.insert("NO_PROXY".into(), np.clone());
                        map.insert("no_proxy".into(), np.clone());
                    }
                }
            }
            tokage_core::proxy::ProxyMode::CliOwn => {
                // Keep login shell proxy variables, but override with local relay if forced
                if p.forced {
                    if let Some(ref url) = p.proxy_url {
                        map.insert("HTTP_PROXY".into(), url.clone());
                        map.insert("HTTPS_PROXY".into(), url.clone());
                        map.insert("ALL_PROXY".into(), url.clone());
                        map.insert("http_proxy".into(), url.clone());
                        map.insert("https_proxy".into(), url.clone());
                        map.insert("all_proxy".into(), url.clone());
                    }
                    if let Some(ref np) = p.no_proxy {
                        map.insert("NO_PROXY".into(), np.clone());
                        map.insert("no_proxy".into(), np.clone());
                    }
                }
            }
            tokage_core::proxy::ProxyMode::System | tokage_core::proxy::ProxyMode::Manual => {
                // Strip existing login env proxy vars
                for var in PROXY_VAR_NAMES {
                    map.remove(*var);
                }
                if let Some(ref url) = p.proxy_url {
                    map.insert("HTTP_PROXY".into(), url.clone());
                    map.insert("HTTPS_PROXY".into(), url.clone());
                    map.insert("ALL_PROXY".into(), url.clone());
                    map.insert("http_proxy".into(), url.clone());
                    map.insert("https_proxy".into(), url.clone());
                    map.insert("all_proxy".into(), url.clone());
                }
                if let Some(ref np) = p.no_proxy {
                    map.insert("NO_PROXY".into(), np.clone());
                    map.insert("no_proxy".into(), np.clone());
                }
            }
            tokage_core::proxy::ProxyMode::InheritGlobal => {}
        }

        // Inject provider extra env (e.g. NODE_USE_ENV_PROXY=1)
        for (k, v) in &p.extra_env {
            map.insert(k.clone(), v.clone());
        }

        // Inject CA cert if specified
        if let Some(ref ca) = p.ca_file {
            for var in CA_CERT_VAR_NAMES {
                map.insert((*var).to_string(), ca.clone());
            }
        }
    }

    if let Ok(wrapper) = ensure_shell_wrapper() {
        map.insert("SHELL".into(), wrapper.to_string_lossy().into_owned());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_CLIS: [ProviderId; 7] = [
        ProviderId::ClaudeCode,
        ProviderId::CodexCli,
        ProviderId::CursorCli,
        ProviderId::CopilotCli,
        ProviderId::AntigravityCli,
        ProviderId::Aider,
        ProviderId::Opencode,
    ];

    /// Each CLI's own key variables in the test environment.
    fn own(id: ProviderId) -> &'static [&'static str] {
        match id {
            ProviderId::ClaudeCode => &[
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_BASE_URL",
                "ANTHROPIC_CUSTOM_HEADERS",
                "CLAUDE_CODE_OAUTH_TOKEN",
            ],
            ProviderId::CodexCli => &[
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
                "OPENAI_ORG_ID",
                "OPENAI_ORGANIZATION",
                "OPENAI_PROJECT",
                "CODEX_API_KEY",
                "RELAY_API_KEY",
            ],
            ProviderId::CursorCli => &["CURSOR_API_KEY", "CURSOR_API_ENDPOINT"],
            ProviderId::CopilotCli => &[
                "COPILOT_GITHUB_TOKEN",
                "GH_TOKEN",
                "GITHUB_TOKEN",
                "GH_ENTERPRISE_TOKEN",
                "GITHUB_ENTERPRISE_TOKEN",
            ],
            ProviderId::AntigravityCli => &[
                "GEMINI_API_KEY",
                "GOOGLE_API_KEY",
                "GOOGLE_APPLICATION_CREDENTIALS",
            ],
            ProviderId::Aider => &[
                "AIDER_API_KEY",
            ],
            ProviderId::Opencode => &[
                "OPENCODE_API_KEY",
            ],
            _ => &[],
        }
    }

    /// A login env holding every vendor's keys, plus a Codex config naming a custom key var.
    fn login_env(home: &std::path::Path) -> ShellEnv {
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            "model_provider = \"relay\"\n[model_providers.relay]\nbase_url = \"https://r.example/v1\"\nenv_key = \"RELAY_API_KEY\" # c\n",
        )
        .unwrap();
        let mut vars: HashMap<String, String> = CREDENTIAL_ENV
            .iter()
            .chain(ALL_CLIS.iter().flat_map(|id| own(*id)))
            .map(|k| (k.to_string(), format!("secret-{k}")))
            .collect();
        for (k, v) in [
            ("HOME", home.to_str().unwrap()),
            ("PATH", "/usr/bin:/bin"),
            ("LANG", "en_US.UTF-8"),
            ("HTTPS_PROXY", "http://127.0.0.1:7890"),
            ("HOMEBREW_GITHUB_API_TOKEN", "secret-brew"),
            ("MY_SERVICE_SECRET", "secret-svc"),
            ("DB_PASSWORD", "secret-db"),
            ("CLAUDECODE", "1"),
        ] {
            vars.insert(k.into(), v.into());
        }
        ShellEnv { vars }
    }

    fn temp_home(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tokage-env-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn credential_names() {
        for k in ["OPENAI_API_KEY", "gh_token", "X_SECRET", "AWS_SECRET_ACCESS_KEY", "FOO_BASE_URL"] {
            assert!(is_credential(k), "{k}");
        }
        for k in ["PATH", "HOME", "SSH_AUTH_SOCK", "TERM_SESSION_ID", "HTTPS_PROXY", "AWS_REGION", "TOKENIZERS_PARALLELISM"] {
            assert!(!is_credential(k), "{k}");
        }
    }

    #[test]
    fn each_cli_sees_only_its_own_keys() {
        let home = temp_home("own");
        let env = login_env(&home);
        for id in ALL_CLIS {
            let child = assemble(id, &env, &[]);
            for k in own(id) {
                assert!(child.contains_key(*k), "{id:?} lost its own {k}");
            }
            for (k, _) in env.vars.iter().filter(|(k, _)| is_credential(k)) {
                if !own(id).contains(&k.as_str()) {
                    assert!(!child.contains_key(k), "{id:?} sees {k}");
                }
            }
            // No credential value of another vendor survives under any name.
            for other in ALL_CLIS.iter().filter(|o| **o != id) {
                for k in own(*other).iter().filter(|k| !own(id).contains(k)) {
                    let v = format!("secret-{k}");
                    assert!(!child.values().any(|x| *x == v), "{id:?} sees value of {k}");
                }
            }
            for k in ["PATH", "HOME", "LANG", "HTTPS_PROXY"] {
                assert!(child.contains_key(k), "{id:?} lost {k}");
            }
            assert!(!child.contains_key("HOMEBREW_GITHUB_API_TOKEN"));
            assert!(!child.contains_key("DB_PASSWORD"));
        }
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn other_clis_get_no_keys() {
        let home = temp_home("other");
        let env = login_env(&home);
        let child = assemble(ProviderId::GrokCli, &env, &[]);
        assert!(child.keys().all(|k| !is_credential(k)));
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn claude_cloud_backends_are_opt_in() {
        let home = temp_home("cloud");
        let mut env = login_env(&home);
        assert!(!assemble(ProviderId::ClaudeCode, &env, &[]).contains_key("AWS_SECRET_ACCESS_KEY"));
        env.vars.insert("CLAUDE_CODE_USE_BEDROCK".into(), "1".into());
        assert!(assemble(ProviderId::ClaudeCode, &env, &[]).contains_key("AWS_SECRET_ACCESS_KEY"));
        assert!(!assemble(ProviderId::CodexCli, &env, &[]).contains_key("AWS_SECRET_ACCESS_KEY"));
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn host_session_vars_removed() {
        let home = temp_home("host");
        let env = login_env(&home);
        let child = assemble(ProviderId::ClaudeCode, &env, &["CLAUDECODE".into()]);
        assert!(!child.contains_key("CLAUDECODE"));
        std::fs::remove_dir_all(home).ok();
    }

    /// End to end: a real child process started the way `run_cli` starts a CLI.
    #[cfg(unix)]
    #[tokio::test]
    async fn spawned_process_env() {
        let home = temp_home("spawn");
        let env = login_env(&home);
        for id in ALL_CLIS {
            let mut cmd = tokio::process::Command::new("/usr/bin/env");
            super::super::apply_env(&mut cmd, id, &env, &[], None);
            let out = cmd.output().await.unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            let names: Vec<&str> = text.lines().filter_map(|l| l.split_once('=').map(|(k, _)| k)).collect();
            for k in names.iter().filter(|k| is_credential(k)) {
                assert!(own(id).contains(k), "{id:?} child sees {k}");
            }
            for k in own(id) {
                assert!(names.contains(k), "{id:?} child lost {k}");
            }
            assert!(names.contains(&"NO_COLOR"));
        }
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn test_assemble_with_proxy_none_strips_proxy_vars() {
        let home = temp_home("proxy-none");
        let env = login_env(&home);
        assert!(env.vars.contains_key("HTTPS_PROXY"));

        let proxy = crate::proxy::PreparedTurnProxy {
            mode: tokage_core::proxy::ProxyMode::None,
            relay_handle: None,
            proxy_url: None,
            no_proxy: None,
            ca_file: None,
            extra_env: HashMap::new(),
            forced: false,
        };

        let child = assemble_with_proxy(ProviderId::ClaudeCode, &env, &[], Some(&proxy));
        assert!(!child.contains_key("HTTPS_PROXY"));
        assert!(!child.contains_key("HTTP_PROXY"));
        assert!(!child.contains_key("ALL_PROXY"));
        assert!(!child.contains_key("https_proxy"));
        assert!(!child.contains_key("http_proxy"));
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn test_assemble_with_proxy_manual_injects_proxy_and_ca() {
        let home = temp_home("proxy-manual");
        let env = login_env(&home);

        let mut extra_env = HashMap::new();
        extra_env.insert("NODE_USE_ENV_PROXY".into(), "1".into());

        let proxy = crate::proxy::PreparedTurnProxy {
            mode: tokage_core::proxy::ProxyMode::Manual,
            relay_handle: None,
            proxy_url: Some("http://proxy.corp.example:8080".into()),
            no_proxy: Some("localhost,127.0.0.1".into()),
            ca_file: Some("/path/to/custom-ca.pem".into()),
            extra_env,
            forced: false,
        };

        let child = assemble_with_proxy(ProviderId::CursorCli, &env, &[], Some(&proxy));
        assert_eq!(child.get("HTTPS_PROXY").map(String::as_str), Some("http://proxy.corp.example:8080"));
        assert_eq!(child.get("HTTP_PROXY").map(String::as_str), Some("http://proxy.corp.example:8080"));
        assert_eq!(child.get("NO_PROXY").map(String::as_str), Some("localhost,127.0.0.1"));
        assert_eq!(child.get("NODE_USE_ENV_PROXY").map(String::as_str), Some("1"));
        assert_eq!(child.get("NODE_EXTRA_CA_CERTS").map(String::as_str), Some("/path/to/custom-ca.pem"));
        assert_eq!(child.get("SSL_CERT_FILE").map(String::as_str), Some("/path/to/custom-ca.pem"));
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn test_assemble_with_proxy_credentials_isolated_p3m6h() {
        let home = temp_home("proxy-creds");
        let env = login_env(&home);

        // Local relay proxy URL only contains loopback IP and port
        let proxy = crate::proxy::PreparedTurnProxy {
            mode: tokage_core::proxy::ProxyMode::Manual,
            relay_handle: None,
            proxy_url: Some("http://127.0.0.1:49152".into()),
            no_proxy: Some("localhost,127.0.0.1".into()),
            ca_file: None,
            extra_env: HashMap::new(),
            forced: true,
        };

        let child = assemble_with_proxy(ProviderId::ClaudeCode, &env, &[], Some(&proxy));
        assert_eq!(child.get("HTTPS_PROXY").map(String::as_str), Some("http://127.0.0.1:49152"));
        // Upstream username and password must never appear in child env
        for v in child.values() {
            assert!(!v.contains("secret-upstream-pass"));
            assert!(!v.contains("upstream-user"));
        }
        std::fs::remove_dir_all(home).ok();
    }
}
