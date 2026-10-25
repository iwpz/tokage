//! Named Aider global DeepSeek connection; never a claim about an arbitrary project/model.
use super::{
    contract::*,
    http,
    identity::{self, Identity},
};
use crate::{env::ShellEnv, online_usage::QueryPermit, proxy::PreparedTurnProxy};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
fn fail(category: QueryError) -> QueryFailure {
    QueryFailure {
        category,
        retry_after: None,
    }
}
struct Source {
    identity: Identity,
    credential: http::UsageCredential,
}
fn selected(name: &str) -> bool {
    matches!(
        name,
        "DEEPSEEK_API_KEY"
            | "DEEPSEEK_API_BASE"
            | "DEEPSEEK_BASE_URL"
            | "OPENAI_API_BASE"
            | "AIDER_API_KEY"
            | "AIDER_SET_ENV"
            | "AIDER_OPENAI_API_BASE"
            | "AIDER_ENV_FILE"
            | "AIDER_CONFIG"
    )
}
fn dotenv(text: &str, vars: &mut HashMap<String, String>) -> Result<(), QueryFailure> {
    for line in text.lines() {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        if line.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !selected(key) {
            continue;
        }
        let raw = raw.trim();
        let value = if raw.starts_with('"') || raw.starts_with('\'') {
            let quote = raw.as_bytes()[0];
            let mut escaped = false;
            let mut end = None;
            for (i, b) in raw.bytes().enumerate().skip(1) {
                if !escaped && b == quote {
                    end = Some(i);
                    break;
                }
                if b == b'\\' && !escaped {
                    escaped = true;
                } else {
                    escaped = false;
                }
            }
            let end = end.ok_or_else(|| fail(QueryError::InvalidData))?;
            let tail = raw[end + 1..].trim();
            if !tail.is_empty() && !tail.starts_with('#') {
                return Err(fail(QueryError::InvalidData));
            }
            if quote == b'"' {
                serde_json::from_str::<String>(&raw[..=end])
                    .map_err(|_| fail(QueryError::InvalidData))?
            } else {
                let s = &raw[1..end];
                if s.contains('\\') {
                    return Err(fail(QueryError::InvalidData));
                }
                s.into()
            }
        } else {
            raw.split_once(" #").map_or(raw, |(v, _)| v).trim().into()
        };
        // No shell evaluation or interpolation guessed from another resolver.
        if value.contains('$') || value.chars().any(char::is_control) {
            return Err(fail(QueryError::InvalidData));
        }
        vars.insert(key.into(), value);
    }
    Ok(())
}
fn text(path: &Path) -> Result<String, QueryFailure> {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => Err(fail(QueryError::Identity)),
        Ok(_) => super::read_bounded(path, 1048576)
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or_else(|| fail(QueryError::InvalidData)),
    }
}
fn global_path(value: &str, home: &Path) -> Result<PathBuf, QueryFailure> {
    let p = if let Some(t) = value.strip_prefix("~/") {
        home.join(t)
    } else {
        PathBuf::from(value)
    };
    if p.is_absolute() {
        Ok(p)
    } else {
        Err(fail(QueryError::Identity))
    }
}
fn list(v: &serde_yaml::Value) -> Result<Vec<String>, QueryFailure> {
    if v.is_null() {
        Ok(vec![])
    } else if let Some(s) = v.as_str() {
        Ok(vec![s.into()])
    } else if let Some(a) = v.as_sequence() {
        a.iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| fail(QueryError::InvalidData))
            })
            .collect()
    } else {
        Err(fail(QueryError::InvalidData))
    }
}
fn option(
    config: &serde_yaml::Value,
    vars: &HashMap<String, String>,
    name: &str,
    env: &str,
) -> Result<Vec<String>, QueryFailure> {
    if let Some(s) = vars.get(env) {
        if s.trim().starts_with('[') {
            list(
                &serde_yaml::from_str::<serde_yaml::Value>(s)
                    .map_err(|_| fail(QueryError::InvalidData))?,
            )
        } else {
            Ok(vec![s.clone()])
        }
    } else {
        list(&config[name])
    }
}
fn source_from(
    exe: &Path,
    home: &Path,
    config: &serde_yaml::Value,
    vars: &HashMap<String, String>,
    salt: &[u8],
) -> Result<Source, QueryFailure> {
    let set = option(config, vars, "set-env", "AIDER_SET_ENV")?;
    let keys = option(config, vars, "api-key", "AIDER_API_KEY")?;
    let mut vars = vars.clone();
    for item in set {
        let (k, v) = item
            .split_once('=')
            .ok_or_else(|| fail(QueryError::InvalidData))?;
        if selected(k.trim()) {
            vars.insert(k.trim().into(), v.trim().into());
        }
    }
    for item in keys {
        let (name, key) = item
            .split_once('=')
            .ok_or_else(|| fail(QueryError::InvalidData))?;
        if name.trim().eq_ignore_ascii_case("deepseek") {
            vars.insert("DEEPSEEK_API_KEY".into(), key.trim().into());
        }
    }
    let official = |v: &str| {
        matches!(
            v,
            "" | "https://api.deepseek.com"
                | "https://api.deepseek.com/"
                | "https://api.deepseek.com/v1"
                | "https://api.deepseek.com/v1/"
        )
    };
    for name in [
        "DEEPSEEK_API_BASE",
        "DEEPSEEK_BASE_URL",
        "OPENAI_API_BASE",
        "AIDER_OPENAI_API_BASE",
    ] {
        if vars.get(name).is_some_and(|v| !official(v)) {
            return Err(fail(QueryError::Identity));
        }
    }
    if config
        .get("openai-api-base")
        .is_some_and(|v| !v.as_str().is_some_and(official))
    {
        return Err(fail(QueryError::Identity));
    }
    let key = vars
        .get("DEEPSEEK_API_KEY")
        .filter(|s| {
            !s.trim().is_empty()
                && !s.starts_with("YOUR_")
                && !s.contains('$')
                && !s.chars().any(char::is_whitespace)
        })
        .ok_or_else(|| fail(QueryError::Authentication))?;
    Ok(Source {
        identity: Identity {
            key: identity::opaque_key(
                salt,
                &[
                    b"aider-global-official-deepseek-wallet",
                    exe.as_os_str().as_encoded_bytes(),
                    home.as_os_str().as_encoded_bytes(),
                    key.as_bytes(),
                ],
            ),
        },
        credential: http::UsageCredential::bearer(key.clone())?,
    })
}
fn source(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Source, QueryFailure> {
    let home = PathBuf::from(
        env.vars
            .get("HOME")
            .ok_or_else(|| fail(QueryError::Identity))?,
    );
    let conf = if let Some(p) = env.vars.get("AIDER_CONFIG") {
        global_path(p, &home)?
    } else {
        home.join(".aider.conf.yml")
    };
    if env.vars.contains_key("AIDER_CONFIG") && !conf.is_file() {
        return Err(fail(QueryError::Identity));
    }
    let raw = text(&conf)?;
    let config = if raw.trim().is_empty() {
        serde_yaml::Value::Null
    } else {
        serde_yaml::from_str::<serde_yaml::Value>(&raw)
            .map_err(|_| fail(QueryError::InvalidData))?
    };
    if !config.is_null() && !config.is_mapping() {
        return Err(fail(QueryError::InvalidData));
    }
    let extra = env
        .vars
        .get("AIDER_ENV_FILE")
        .map(String::as_str)
        .or_else(|| config["env-file"].as_str())
        .map(|p| global_path(p, &home))
        .transpose()?;
    let mut vars: HashMap<_, _> = env
        .vars
        .iter()
        .filter(|(k, _)| selected(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for path in [
        Some(home.join(".aider/oauth-keys.env")),
        Some(home.join(".env")),
        extra,
    ]
    .into_iter()
    .flatten()
    {
        dotenv(&text(&path)?, &mut vars)?;
    }
    for name in ["AIDER_CONFIG", "AIDER_ENV_FILE"] {
        if vars.get(name) != env.vars.get(name) {
            return Err(fail(QueryError::Identity));
        }
    }
    source_from(exe, &home, &config, &vars, salt)
}
pub(crate) fn resolve(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<Identity> {
    source(exe, env, salt).ok().map(|s| s.identity)
}
pub(crate) async fn query(
    permit: &QueryPermit,
    exe: &Path,
    env: &ShellEnv,
    salt: &[u8],
    expected: &Identity,
    proxy: &PreparedTurnProxy,
) -> Result<QueryOutput, QueryFailure> {
    let src = source(exe, env, salt)?;
    if &src.identity != expected {
        return Err(fail(QueryError::Identity));
    }
    let value = http::get(
        permit,
        http::UsageHttpTarget::DeepSeekBalance,
        src.credential,
        env,
        proxy,
    )
    .await?;
    super::opencode::parse(&value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precedence_and_overrides_never_send_a_gateway_key_to_official_wallet() {
        let mut vars = HashMap::from([("DEEPSEEK_API_KEY".into(), "environment".into())]);
        dotenv("export DEEPSEEK_API_KEY='dotenv' # global\n", &mut vars).unwrap();
        assert_eq!(vars["DEEPSEEK_API_KEY"], "dotenv");
        let config: serde_yaml::Value = serde_yaml::from_str(
            "set-env: [DEEPSEEK_API_KEY=setter]\napi-key: [deepseek=explicit]\n",
        )
        .unwrap();
        let a = source_from(
            Path::new("/cli"),
            Path::new("/home"),
            &config,
            &vars,
            b"salt",
        )
        .unwrap();
        let direct = source_from(
            Path::new("/cli"),
            Path::new("/home"),
            &serde_yaml::Value::Null,
            &HashMap::from([("DEEPSEEK_API_KEY".into(), "explicit".into())]),
            b"salt",
        )
        .unwrap();
        assert!(a.identity == direct.identity);
        vars.insert("DEEPSEEK_API_BASE".into(), "https://gateway.invalid".into());
        assert_eq!(
            source_from(
                Path::new("/cli"),
                Path::new("/home"),
                &config,
                &vars,
                b"salt"
            )
            .err()
            .unwrap()
            .category,
            QueryError::Identity
        );
        vars.remove("DEEPSEEK_API_BASE");vars.insert("OPENAI_API_BASE".into(),"https://gateway.invalid".into());
        assert!(source_from(Path::new("/cli"),Path::new("/home"),&config,&vars,b"salt").is_err());
        assert!(dotenv("DEEPSEEK_API_KEY=${OTHER}", &mut vars).is_err());
    }
    #[test]
    fn global_files_and_rotation_are_separate_from_project_selection() {
        let dir = tempfile::tempdir().unwrap();
        let env = ShellEnv {
            vars: [("HOME".into(), dir.path().display().to_string())].into(),
        };
        let exe = Path::new("/aider");
        std::fs::write(dir.path().join(".env"), "DEEPSEEK_API_KEY=first\n").unwrap();
        let a = resolve(exe, &env, b"s").unwrap();
        std::fs::write(dir.path().join(".env"), "DEEPSEEK_API_KEY=second\n").unwrap();
        assert!(a != resolve(exe, &env, b"s").unwrap());
        std::fs::write(
            dir.path().join(".aider.conf.yml"),
            "api-key: [deepseek=explicit]\n",
        )
        .unwrap();
        let b = resolve(exe, &env, b"s").unwrap();
        std::fs::write(dir.path().join(".env"), "DEEPSEEK_API_KEY=third\n").unwrap();
        assert!(b == resolve(exe, &env, b"s").unwrap());
        std::fs::write(
            dir.path().join(".aider.conf.yml"),
            "env-file: project.env\n",
        )
        .unwrap();
        assert!(resolve(exe, &env, b"s").is_none());
    }
    #[tokio::test]
    #[ignore = "installed Aider named DeepSeek wallet diagnostic; no model requests"]
    async fn installed_wallet() {
        let env = crate::env::capture().await;
        let exe = which::which_in("aider", env.path(), std::env::current_dir().unwrap()).unwrap();
        let identity = resolve(&exe, &env, b"diagnostic").expect("global official DeepSeek source");
        let proxy = crate::proxy::prepare_turn_proxy(
            &Default::default(),
            tokage_core::ProviderId::Aider,
            &env,
        )
        .await
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = crate::online_usage::OnlineUsagePermission::open(&dir.path().join("consent"));
        p.set(true).unwrap();
        let q = query(
            &p.permit().unwrap(),
            &exe,
            &env,
            b"diagnostic",
            &identity,
            &proxy,
        )
        .await
        .map_err(|e| e.category)
        .unwrap();
        assert!(q.windows.is_empty());
        println!(
            "PASS Aider named wallet: {} currency quantities; no CLI chat",
            q.quantities.len()
        );
    }
}
