//! Read only the V1 global connection. No version-specific unknown command or model prompt.
use super::*;
use std::path::PathBuf;
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, QueryFailure> {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(failure(QueryError::Identity)),
        Ok(_) => super::super::read_bounded(path, 1048576)
            .map(Some)
            .ok_or_else(|| failure(QueryError::InvalidData)),
    }
}
fn bounded_nesting(text: &str) -> Result<(), QueryFailure> {
    let b = text.as_bytes();
    let (mut i, mut depth, mut state, mut escaped) = (0, 0u32, 0u8, false);
    while i < b.len() {
        let c = b[i];
        match state {
            1 => {
                if escaped {
                    escaped = false;
                } else if c == b'\\' {
                    escaped = true;
                } else if c == b'"' {
                    state = 0;
                }
            }
            2 => {
                if c == b'\n' {
                    state = 0;
                }
            }
            3 => {
                if c == b'*' && b.get(i + 1) == Some(&b'/') {
                    state = 0;
                    i += 1;
                }
            }
            _ => {
                if c == b'"' {
                    state = 1;
                } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
                    state = 2;
                    i += 1;
                } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
                    state = 3;
                    i += 1;
                } else if c == b'{' || c == b'[' {
                    depth += 1;
                    if depth > 96 {
                        return Err(failure(QueryError::InvalidData));
                    }
                } else if c == b'}' || c == b']' {
                    depth = depth.saturating_sub(1);
                }
            }
        }
        i += 1;
    }
    Ok(())
}
fn jsonc(text: &str) -> Result<Value, QueryFailure> {
    bounded_nesting(text)?;
    jsonc_parser::parse_to_serde_value(
        text,
        &jsonc_parser::ParseOptions {
            allow_comments: true,
            allow_trailing_commas: true,
            allow_loose_object_property_names: false,
        },
    )
    .map_err(|_| failure(QueryError::InvalidData))?
    .filter(Value::is_object)
    .ok_or_else(|| failure(QueryError::InvalidData))
}
fn merge(to: &mut Value, from: Value) {
    if let (Some(a), Some(b)) = (to.as_object_mut(), from.as_object()) {
        for (k, v) in b {
            if a.get(k).is_some_and(Value::is_object) && v.is_object() {
                merge(a.get_mut(k).unwrap(), v.clone());
            } else {
                a.insert(k.clone(), v.clone());
            }
        }
    }
}
fn path(value: &str) -> Result<PathBuf, QueryFailure> {
    let p = PathBuf::from(value);
    if p.is_absolute() {
        Ok(p)
    } else {
        Err(failure(QueryError::Identity))
    }
}
fn value(v: &Value, env: &ShellEnv) -> Result<String, QueryFailure> {
    let s = v.as_str().ok_or_else(|| failure(QueryError::InvalidData))?;
    if let Some(name) = s.strip_prefix("{env:").and_then(|s| s.strip_suffix('}')) {
        if !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            return env
                .vars
                .get(name)
                .cloned()
                .ok_or_else(|| failure(QueryError::Authentication));
        }
    }
    if s.contains('{') || s.contains('}') {
        return Err(failure(QueryError::Identity));
    }
    Ok(s.into())
}
fn from_values(
    auth: &Value,
    config: &Value,
    exe: &Path,
    env: &ShellEnv,
    root: &Path,
    salt: &[u8],
) -> Result<Source, QueryFailure> {
    let map = auth
        .as_object()
        .ok_or_else(|| failure(QueryError::InvalidData))?;
    if map
        .iter()
        .any(|(k, v)| k.starts_with("http:") || k.starts_with("https:") || v["type"] == "wellknown")
        || config["enterprise"].is_object()
        || config["plugin"].as_array().is_some_and(|p| !p.is_empty())
        || config["disabled_providers"]
            .as_array()
            .is_some_and(|p| p.iter().any(|s| s == "deepseek"))
        || config["enabled_providers"]
            .as_array()
            .is_some_and(|p| !p.iter().any(|s| s == "deepseek"))
    {
        return Err(failure(QueryError::Identity));
    }
    let provider = &config["provider"]["deepseek"];
    if provider.get("npm").is_some_and(|p| {
        !matches!(
            p.as_str(),
            Some("@ai-sdk/deepseek" | "@ai-sdk/openai-compatible")
        )
    }) || provider
        .get("headers")
        .is_some_and(|p| !p.is_null() && !p.as_object().is_some_and(|m| m.is_empty()))
    {
        return Err(failure(QueryError::Identity));
    }
    if !provider.is_null() && !provider.is_object() {
        return Err(failure(QueryError::InvalidData));
    }
    let options = &provider["options"];
    if !options.is_null() && !options.is_object() {
        return Err(failure(QueryError::InvalidData));
    }
    if options.as_object().is_some_and(|m| {
        m.keys()
            .any(|k| !matches!(k.as_str(), "baseURL" | "apiKey" | "timeout" | "maxRetries"))
    }) {
        return Err(failure(QueryError::Identity));
    }
    if let Some(base) = options.get("baseURL") {
        if !official(&Value::String(value(base, env)?)) {
            return Err(failure(QueryError::Identity));
        }
    }
    for name in ["DEEPSEEK_API_BASE", "DEEPSEEK_BASE_URL"] {
        if env
            .vars
            .get(name)
            .is_some_and(|s| !official(&Value::String(s.clone())))
        {
            return Err(failure(QueryError::Identity));
        }
    }
    let (origin, key) = if let Some(key) = options.get("apiKey") {
        ("global-config", value(key, env)?)
    } else if let Some(row) = auth.get("deepseek") {
        if row["type"] != "api" {
            return Err(failure(QueryError::Identity));
        }
        (
            "saved-api",
            row["key"]
                .as_str()
                .ok_or_else(|| failure(QueryError::InvalidData))?
                .into(),
        )
    } else {
        (
            "environment",
            env.vars
                .get("DEEPSEEK_API_KEY")
                .cloned()
                .ok_or_else(|| failure(QueryError::Authentication))?,
        )
    };
    let credential = http::UsageCredential::bearer(key.clone())?;
    Ok(Source {
        identity: Identity {
            key: identity::opaque_key(
                salt,
                &[
                    b"opencode-v1-global-official-deepseek-wallet",
                    exe.as_os_str().as_encoded_bytes(),
                    root.as_os_str().as_encoded_bytes(),
                    origin.as_bytes(),
                    key.as_bytes(),
                ],
            ),
        },
        credential,
    })
}
fn load_root(root: &Path, config: &mut Value) -> Result<(), QueryFailure> {
    for name in ["plugins", "plugin"] {
        match std::fs::read_dir(root.join(name)) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    return Err(failure(QueryError::Identity));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(failure(QueryError::Identity)),
        }
    }
    for name in ["config.json", "opencode.json", "opencode.jsonc"] {
        if let Some(bytes) = read_optional(&root.join(name))? {
            merge(
                config,
                jsonc(std::str::from_utf8(&bytes).map_err(|_| failure(QueryError::InvalidData))?)?,
            );
        }
    }
    Ok(())
}
fn reject_managed(env: &ShellEnv) -> Result<(), QueryFailure> {
    #[cfg(target_os = "macos")]
    let mut files = vec![
        PathBuf::from("/Library/Application Support/opencode/opencode.json"),
        PathBuf::from("/Library/Application Support/opencode/opencode.jsonc"),
        PathBuf::from("/Library/Managed Preferences/ai.opencode.managed.plist"),
    ];
    #[cfg(target_os = "macos")]
    if let Some(user) = env.vars.get("USER").filter(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    }) {
        files.push(
            PathBuf::from("/Library/Managed Preferences")
                .join(user)
                .join("ai.opencode.managed.plist"),
        );
    }
    #[cfg(target_os = "linux")]
    let files = vec![
        PathBuf::from("/etc/opencode/opencode.json"),
        PathBuf::from("/etc/opencode/opencode.jsonc"),
    ];
    #[cfg(target_os = "windows")]
    let files = vec![
        PathBuf::from(
            env.vars
                .get("PROGRAMDATA")
                .map(String::as_str)
                .unwrap_or("C:/ProgramData"),
        )
        .join("opencode/opencode.json"),
        PathBuf::from(
            env.vars
                .get("PROGRAMDATA")
                .map(String::as_str)
                .unwrap_or("C:/ProgramData"),
        )
        .join("opencode/opencode.jsonc"),
    ];
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let files: Vec<PathBuf> = vec![];
    let _ = env;
    for path in files {
        match std::fs::symlink_metadata(path) {
            Ok(_) => return Err(failure(QueryError::Identity)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(failure(QueryError::Identity)),
        }
    }
    Ok(())
}
pub(super) fn source(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Source, QueryFailure> {
    let home = PathBuf::from(
        env.vars
            .get("HOME")
            .ok_or_else(|| failure(QueryError::Identity))?,
    );
    let data = env
        .vars
        .get("XDG_DATA_HOME")
        .map(|p| path(p))
        .transpose()?
        .unwrap_or_else(|| home.join(".local/share"))
        .join("opencode");
    let conf = env
        .vars
        .get("XDG_CONFIG_HOME")
        .map(|p| path(p))
        .transpose()?
        .unwrap_or_else(|| home.join(".config"))
        .join("opencode");
    reject_managed(env)?;
    let auth = if let Some(content) = env.vars.get("OPENCODE_AUTH_CONTENT") {
        serde_json::from_str(content).map_err(|_| failure(QueryError::InvalidData))?
    } else {
        match read_optional(&data.join("auth.json"))? {
            Some(b) => serde_json::from_slice(&b).map_err(|_| failure(QueryError::InvalidData))?,
            None => serde_json::json!({}),
        }
    };
    let mut config = serde_json::json!({});
    load_root(&conf, &mut config)?;
    if let Some(file) = env.vars.get("OPENCODE_CONFIG") {
        let bytes = read_optional(&path(file)?)?.ok_or_else(|| failure(QueryError::Identity))?;
        merge(
            &mut config,
            jsonc(std::str::from_utf8(&bytes).map_err(|_| failure(QueryError::InvalidData))?)?,
        );
    }
    if let Some(dir) = env.vars.get("OPENCODE_CONFIG_DIR") {
        load_root(&path(dir)?, &mut config)?;
    }
    if let Some(content) = env.vars.get("OPENCODE_CONFIG_CONTENT") {
        merge(&mut config, jsonc(content)?);
    }
    from_values(&auth, &config, exe, env, &data, salt)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn global_jsonc_is_strict_and_auth_config_environment_precedence_is_bound() {
        let env = ShellEnv {
            vars: [("DEEPSEEK_API_KEY".into(), "environment".into())].into(),
        };
        let exe = Path::new("/cli");
        let root = Path::new("/data");
        let auth = serde_json::json!({"deepseek":{"type":"api","key":"saved"}});
        let config=jsonc("{ // supported comment\n \"provider\": {\"deepseek\":{\"options\":{\"baseURL\":\"https://api.deepseek.com\",},},},}").unwrap();
        let a = from_values(&auth, &config, exe, &env, root, b"s").unwrap();
        let b = from_values(&serde_json::json!({}), &config, exe, &env, root, b"s").unwrap();
        assert!(a.identity != b.identity);
        let mut override_config = config.clone();
        override_config["provider"]["deepseek"]["options"]["apiKey"] =
            serde_json::json!("explicit");
        let c = from_values(&auth, &override_config, exe, &env, root, b"s").unwrap();
        assert!(c.identity != a.identity);
        assert!(jsonc("{unquoted:1}").is_err());
        for url in [
            "https://gateway.invalid",
            "https://api.deepseek.com.evil.invalid",
            "{file:/secret}",
        ] {
            let mut v = config.clone();
            v["provider"]["deepseek"]["options"]["baseURL"] = Value::String(url.into());
            assert!(from_values(&auth, &v, exe, &env, root, b"s").is_err());
        }
    }
    #[test]
    fn nested_config_is_bounded_without_counting_strings_or_comments() {
        assert!(jsonc(&format!(
            "{{\"x\":{}0{}}}",
            "[".repeat(200),
            "]".repeat(200)
        ))
        .is_err());
        let v =
            serde_json::json!({"instructions":["[".repeat(300)],"url":"https://api.deepseek.com"});
        assert!(jsonc(&v.to_string()).is_ok());
    }
    #[test]
    fn global_config_precedence_and_inline_auth_do_not_borrow_an_unrelated_source() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join(".config/opencode");
        let custom = dir.path().join("extra");
        std::fs::create_dir_all(&global).unwrap();
        std::fs::create_dir_all(&custom).unwrap();
        let config = |key: &str| {
            serde_json::json!({"provider":{"deepseek":{"options":{"apiKey":key}}}}).to_string()
        };
        std::fs::write(global.join("opencode.json"), config("global")).unwrap();
        std::fs::write(custom.join("opencode.jsonc"), config("custom-dir")).unwrap();
        let file = dir.path().join("custom.json");
        std::fs::write(&file, config("custom-file")).unwrap();
        let mut env = ShellEnv {
            vars: [
                ("HOME".into(), dir.path().display().to_string()),
                ("OPENCODE_CONFIG".into(), file.display().to_string()),
                ("OPENCODE_CONFIG_DIR".into(), custom.display().to_string()),
                ("OPENCODE_CONFIG_CONTENT".into(), config("inline")),
                (
                    "OPENCODE_AUTH_CONTENT".into(),
                    r#"{"deepseek":{"type":"api","key":"saved"}}"#.into(),
                ),
            ]
            .into(),
        };
        let exe = Path::new("/cli");
        let root = dir.path().join(".local/share/opencode");
        let expected = from_values(
            &serde_json::json!({}),
            &serde_json::from_str::<Value>(&config("inline")).unwrap(),
            exe,
            &env,
            &root,
            b"s",
        )
        .unwrap();
        assert!(source(exe, &env, b"s").unwrap().identity == expected.identity);
        env.vars.remove("OPENCODE_CONFIG_CONTENT");
        let from_dir = source(exe, &env, b"s").unwrap().identity;
        let expected = from_values(
            &serde_json::json!({}),
            &serde_json::from_str::<Value>(&config("custom-dir")).unwrap(),
            exe,
            &env,
            &root,
            b"s",
        )
        .unwrap();
        assert!(from_dir == expected.identity);
        env.vars.remove("OPENCODE_CONFIG_DIR");
        assert!(from_dir != source(exe, &env, b"s").unwrap().identity);
        std::fs::create_dir(global.join("plugins")).unwrap();
        std::fs::write(global.join("plugins/unknown.ts"), "").unwrap();
        assert!(source(exe, &env, b"s").is_err());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn v1_uses_only_version_command_never_unknown_v2_management_or_chat() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("opencode");
        std::fs::write(&exe,"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 1.19.0; else touch \"$HOME/unexpected\"; exit 1; fi\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let root = dir.path().join(".local/share/opencode");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("auth.json"),
            r#"{"deepseek":{"type":"api","key":"synthetic"}}"#,
        )
        .unwrap();
        let env = ShellEnv {
            vars: [
                ("HOME".into(), dir.path().display().to_string()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ]
            .into(),
        };
        assert!(super::super::source(&exe, &env, b"salt").await.is_ok());
        assert!(!dir.path().join("unexpected").exists());
    }
}
