//! Kiro account usage through the official CLI’s built-in /usage command.
use super::{
    contract::*,
    identity::{self, Identity},
};
use crate::{env::ShellEnv, proxy::PreparedTurnProxy};
use std::path::{Path, PathBuf};
fn failure(category: QueryError) -> QueryFailure {
    QueryFailure {
        category,
        retry_after: None,
    }
}

struct SocialAccount {
    db: PathBuf,
    provider: &'static str,
    profile: String,
}

// Only account metadata crosses the SQLite boundary. The CLI retains the grants.
fn local_source(env: &ShellEnv) -> Option<SocialAccount> {
    if env
        .vars
        .get("KIRO_API_KEY")
        .is_some_and(|v| !v.trim().is_empty())
    {
        return None;
    }
    let home = PathBuf::from(env.vars.get("HOME")?);
    #[cfg(target_os = "macos")]
    let db = home.join("Library/Application Support/kiro-cli/data.sqlite3");
    #[cfg(not(target_os = "macos"))]
    let db = home.join(".local/share/kiro-cli/data.sqlite3");
    let conn = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.busy_timeout(std::time::Duration::from_millis(200))
        .ok()?;
    let (provider, profile): (String, String) = conn
        .query_row(
            "SELECT json_extract(value, '$.provider'), json_extract(value, '$.profile_arn')
         FROM auth_kv
         WHERE key = 'kirocli:social:token' AND length(value) <= 65536
           AND json_type(value, '$.refresh_token') = 'text'
           AND length(json_extract(value, '$.refresh_token')) > 0
           AND (SELECT count(*) FROM auth_kv) = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()?;
    let provider = match provider.as_str() {
        "google" => "google",
        "github" => "github",
        _ => return None,
    };
    let parts: Vec<_> = profile.split(':').collect();
    let ["arn", "aws", "codewhisperer", "us-east-1" | "eu-central-1", account, resource] =
        parts.as_slice()
    else {
        return None;
    };
    let suffix = resource.strip_prefix("profile/")?;
    if account.len() != 12
        || !account.bytes().all(|b| b.is_ascii_digit())
        || suffix.is_empty()
        || suffix.len() > 128
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    Some(SocialAccount {
        db,
        provider,
        profile,
    })
}

pub(crate) fn resolve(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<Identity> {
    let source = local_source(env)?;
    Some(Identity {
        key: identity::opaque_key(
            salt,
            &[
                // A previously cached HTTP receipt must not become current CLI usage.
                b"kiro-social-cli-usage",
                exe.as_os_str().as_encoded_bytes(),
                source.db.as_os_str().as_encoded_bytes(),
                source.provider.as_bytes(),
                source.profile.as_bytes(),
            ],
        ),
    })
}

/// Configured CLI-owned social login evidence.
pub(crate) fn login_label(env: &ShellEnv) -> Option<&'static str> {
    match local_source(env)?.provider {
        "google" => Some("Google account"),
        "github" => Some("GitHub account"),
        _ => None,
    }
}
fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return false;
    }
    let y: u32 = s[..4].parse().unwrap_or(0);
    let m: usize = s[5..7].parse().unwrap_or(0);
    let d: u32 = s[8..].parse().unwrap_or(0);
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = [
        0,
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    y >= 1970 && m > 0 && m <= 12 && d > 0 && d <= days[m]
}
pub fn parse(text: &str) -> Result<QueryOutput, QueryFailure> {
    let text = crate::chat::filter::OutputFilter::new(&ShellEnv {
        vars: Default::default(),
    })
    .strip_ansi(text);
    let mut plan = None;
    let mut reset = None;
    let mut credits = None;
    let mut percent = None;
    let amounts = regex::Regex::new(
        r"^Credits\s*\(([0-9]+(?:\.[0-9]+)?) of ([0-9]+(?:\.[0-9]+)?) covered in plan\)$",
    )
    .unwrap();
    let pct = regex::Regex::new(r"(?:^|\s)([0-9]+(?:\.[0-9]+)?)%\s*$").unwrap();
    for l in text.lines().map(str::trim) {
        if let Some(rest) = l.strip_prefix("Estimated Usage | resets on ") {
            if plan.is_some() {
                return Err(failure(QueryError::InvalidData));
            }
            let (date, p) = rest
                .split_once('|')
                .ok_or_else(|| failure(QueryError::InvalidData))?;
            let date = date.trim();
            if !valid_date(date) {
                return Err(failure(QueryError::InvalidData));
            }
            // Date-only source: retain as text, don't invent a timezone or timestamp.
            reset = Some(date.to_string());
            plan = Some(p.trim().to_string());
        } else if let Some(c) = amounts.captures(l) {
            if credits.is_some() {
                return Err(failure(QueryError::InvalidData));
            }
            credits = Some((c[1].to_string(), c[2].to_string()));
        } else if credits.is_some() && percent.is_none() {
            if let Some(c) = pct.captures(l) {
                percent = c[1]
                    .parse::<f64>()
                    .ok()
                    .filter(|p| (0.0..=100.0).contains(p));
            }
        }
    }
    let plan = plan
        .filter(|p| !p.is_empty() && p.len() < 100)
        .ok_or_else(|| failure(QueryError::InvalidData))?;
    let reset = reset.ok_or_else(|| failure(QueryError::InvalidData))?;
    let (used, limit) = credits.ok_or_else(|| failure(QueryError::InvalidData))?;
    if !decimal(&used) || !decimal(&limit) || limit.parse::<f64>().ok().is_none_or(|x| x <= 0.0) {
        return Err(failure(QueryError::InvalidData));
    }
    let percent = percent.ok_or_else(|| failure(QueryError::InvalidData))?;
    Ok(QueryOutput {
        available: None,
        source: QuerySource::Cli,
        windows: vec![QuotaWindow {
            id: "estimated-plan-credits".into(),
            scope: "plan:estimated".into(),
            period: format!("Estimated · {plan} · {reset}"),
            window_minutes: None,
            used_percent: Some(percent),
            resets_at: None,
        }],
        quantities: vec![Quantity {
            id: "estimated-plan-credits".into(),
            unit: "credits".into(),
            currency: None,
            used: Some(used),
            limit: Some(limit),
            remaining: None,
        }],
        display: None,
    })
}

pub(crate) async fn query(
    permit: &crate::online_usage::QueryPermit,
    exe: &Path,
    env: &ShellEnv,
    proxy: &PreparedTurnProxy,
) -> Result<QueryOutput, QueryFailure> {
    permit
        .run(async { Ok(query_cli(exe, env, proxy).await) })
        .await
        .map_err(|_| failure(QueryError::Cancelled))?
}

/// A fresh normal workspace/profile prevents user agents, resources and MCP startup.
/// CLI-owned HOME and authentication remain unchanged.
async fn query_cli(
    exe: &Path,
    env: &ShellEnv,
    proxy: &PreparedTurnProxy,
) -> Result<QueryOutput, QueryFailure> {
    use tokio::io::AsyncReadExt;
    let dir = tempfile::tempdir().map_err(|_| failure(QueryError::Persistence))?;
    let name = format!(
        "tokage-usage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let agents = dir.path().join(".kiro/agents");
    std::fs::create_dir_all(&agents).map_err(|_| failure(QueryError::Persistence))?;
    std::fs::create_dir_all(dir.path().join(".kiro/settings"))
        .map_err(|_| failure(QueryError::Persistence))?;
    let config = serde_json::json!({"name":name,"description":"Read built-in account usage only","tools":[],"allowedTools":[],"resources":[],"hooks":{},"mcpServers":{},"includeMcpJson":false,"includePowers":false});
    std::fs::write(agents.join(format!("{name}.json")), config.to_string())
        .map_err(|_| failure(QueryError::Persistence))?;
    std::fs::write(
        dir.path().join(".kiro/settings/cli.json"),
        r#"{"chat.disableInheritingDefaultResources":true}"#,
    )
    .map_err(|_| failure(QueryError::Persistence))?;
    let args = [
        "chat",
        "--agent-engine",
        "v1",
        "--legacy-ui",
        "--model",
        "auto",
        "--agent",
        &name,
        "--no-interactive",
        "/usage",
    ]
    .map(String::from);
    let mut cmd = crate::sandbox::create_usage_command(
        tokage_core::ProviderId::KiroCli,
        exe,
        &args,
        &dir.path().to_string_lossy(),
        env.vars.get("HOME").map(String::as_str),
    )
    .map_err(|_| failure(QueryError::Persistence))?;
    crate::chat::apply_env(
        &mut cmd,
        tokage_core::ProviderId::KiroCli,
        env,
        &[],
        Some(proxy),
    );
    cmd.current_dir(dir.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    crate::proc::isolate(&mut cmd);
    let mut child = cmd.spawn().map_err(|_| failure(QueryError::Network))?;
    let _group = crate::proc::Group::new(&child);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| failure(QueryError::Network))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| failure(QueryError::Network))?;
    let read = async move {
        let mut out = vec![];
        let mut err = vec![];
        let mut stdout = stdout.take(65537);
        let mut stderr = stderr.take(65537);
        let (a, b) = tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
        a.map_err(|_| ())?;
        b.map_err(|_| ())?;
        Ok::<_, ()>((out, err))
    };
    let (status, output) = tokio::time::timeout(std::time::Duration::from_secs(120), async {
        tokio::join!(child.wait(), read)
    })
    .await
    .map_err(|_| failure(QueryError::Timeout))?;
    let status = status.map_err(|_| failure(QueryError::Network))?;
    let (out, err) = output.map_err(|_| failure(QueryError::Network))?;
    if out.len() > 65536 || err.len() > 65536 {
        return Err(failure(QueryError::InvalidData));
    }
    // Legacy management renders to stderr; don't treat banners or exit 0 as a quota receipt.
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    if !status.success() {
        return Err(QueryFailure::cli(&text));
    }
    parse(&text)
}
#[cfg(test)]
mod tests {
    use super::*;
    const PANEL: &str = "Estimated Usage | resets on 2026-11-01 | KIRO PRO+\nCredits (123.40 of 2000 covered in plan)\n████ 6.2%\nAdditional credits\nAdd-on credits available for purchase";

    #[test]
    fn estimated_credits_keep_precision_and_date_only_reset() {
        let q = parse(PANEL).unwrap();
        assert_eq!(q.quantities[0].used.as_deref(), Some("123.40"));
        assert_eq!(q.windows[0].used_percent, Some(6.2));
        assert!(q.windows[0].resets_at.is_none());
        assert_eq!(q.windows[0].scope, "plan:estimated");
        assert!(q.windows[0].period.contains("2026-11-01"));
    }
    #[test]
    fn malformed_or_non_usage_output_is_not_success() {
        for s in [
            "Logged in with Google".into(),
            PANEL.replace("6.2%", "101%"),
            PANEL.replace("2000", "0"),
            PANEL.replace("2026-11-01", "bad"),
            PANEL.replace("2026-11-01", "2026-02-30"),
            PANEL.replace("Credits (", "Tokens ("),
            format!("{PANEL}\n{PANEL}"),
        ] {
            assert!(parse(&s).is_err());
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn queries_only_builtin_usage_and_honors_revocation() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("kiro-cli");
        std::fs::write(&cli, r#"#!/bin/sh
case "$*" in
    "chat --agent-engine v1 --legacy-ui --model auto --agent tokage-usage-"*" --no-interactive /usage") ;;
    *) exit 71 ;;
esac
if [ -f "$HOME/reject" ]; then
    echo 'Not logged in' >&2
    exit 1
fi
printf 'Estimated Usage | resets on 2026-11-01 | KIRO PRO+\nCredits (1 of 50 covered in plan)\n2%%\n' >&2
"#).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        let curl = dir.path().join("curl");
        std::fs::write(&curl, "#!/bin/sh\ntouch \"$HOME/http-called\"\nexit 72\n").unwrap();
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o700)).unwrap();
        let env = ShellEnv {
            vars: [
                ("HOME".into(), dir.path().display().to_string()),
                (
                    "PATH".into(),
                    format!("{}:/usr/bin:/bin", dir.path().display()),
                ),
            ]
            .into(),
        };
        let permission =
            crate::online_usage::OnlineUsagePermission::open(&dir.path().join("consent"));
        permission.set(true).unwrap();
        let permit = permission.permit().unwrap();
        let proxy = crate::proxy::prepare_turn_proxy(
            &Default::default(),
            tokage_core::ProviderId::KiroCli,
            &env,
        )
        .await
        .unwrap();
        let output = query(&permit, &cli, &env, &proxy).await.unwrap();
        assert!(matches!(output.source, QuerySource::Cli));
        assert_eq!(output.quantities[0].used.as_deref(), Some("1"));
        assert!(!dir.path().join("http-called").exists());
        std::fs::write(dir.path().join("reject"), "").unwrap();
        assert_eq!(
            query(&permit, &cli, &env, &proxy)
                .await
                .err()
                .unwrap()
                .category,
            QueryError::Authentication
        );
        permission.set(false).unwrap();
        assert_eq!(
            query(&permit, &cli, &env, &proxy)
                .await
                .err()
                .unwrap()
                .category,
            QueryError::Cancelled
        );
    }
    #[test]
    fn read_only_store_rejects_ambiguous_auth_and_api_key_override() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(target_os = "macos")]
        let path = dir
            .path()
            .join("Library/Application Support/kiro-cli/data.sqlite3");
        #[cfg(not(target_os = "macos"))]
        let path = dir.path().join(".local/share/kiro-cli/data.sqlite3");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute("CREATE TABLE auth_kv (key TEXT,value TEXT)", [])
            .unwrap();
        db.execute(
            "INSERT INTO auth_kv VALUES (?1,?2)",
            rusqlite::params![
                "kirocli:social:token",
                r#"{"provider":"google","profile_arn":"arn:aws:codewhisperer:us-east-1:123456789012:profile/synthetic","refresh_token":"synthetic"}"#
            ],
        )
        .unwrap();
        let mut env = ShellEnv {
            vars: [("HOME".into(), dir.path().to_string_lossy().into_owned())].into(),
        };
        assert!(resolve(Path::new("/cli"), &env, b"salt").is_some());
        assert_eq!(login_label(&env), Some("Google account"));
        let identity = resolve(Path::new("/cli"), &env, b"salt").unwrap();
        db.execute(
            "UPDATE auth_kv SET value = json_set(value, '$.refresh_token', 'rotated')",
            [],
        )
        .unwrap();
        assert!(Some(identity.clone()) == resolve(Path::new("/cli"), &env, b"salt"));
        db.execute("UPDATE auth_kv SET value = json_set(value, '$.profile_arn', 'arn:aws:codewhisperer:us-east-1:123456789012:profile/other')", []).unwrap();
        assert!(Some(identity) != resolve(Path::new("/cli"), &env, b"salt"));
        env.vars
            .insert("KIRO_API_KEY".into(), "synthetic-override".into());
        assert!(resolve(Path::new("/cli"), &env, b"salt").is_none());
        assert!(login_label(&env).is_none());
        env.vars.remove("KIRO_API_KEY");
        db.execute("INSERT INTO auth_kv VALUES ('other-auth','{}')", [])
            .unwrap();
        assert!(resolve(Path::new("/cli"), &env, b"salt").is_none());
    }
    #[tokio::test]
    #[ignore = "installed Kiro account management diagnostic; no model prompt; not desktop acceptance"]
    async fn installed_management_query() {
        let env = crate::env::capture().await;
        let exe =
            which::which_in("kiro-cli", env.path(), std::env::current_dir().unwrap()).unwrap();
        let before = resolve(&exe, &env, b"diagnostic").expect("supported local social login");
        let proxy = crate::proxy::prepare_turn_proxy(
            &Default::default(),
            tokage_core::ProviderId::KiroCli,
            &env,
        )
        .await
        .unwrap();
        let started = std::time::Instant::now();
        let dir = tempfile::tempdir().unwrap();
        let permission =
            crate::online_usage::OnlineUsagePermission::open(&dir.path().join("consent"));
        permission.set(true).unwrap();
        let q = query(&permission.permit().unwrap(), &exe, &env, &proxy)
            .await
            .map_err(|e| e.category)
            .expect("built-in usage");
        assert!(Some(before) == resolve(&exe, &env, b"diagnostic"));
        println!("PASS management query: {} scoped windows, {} credit quantities, {}ms; identity unchanged",q.windows.len(),q.quantities.len(),started.elapsed().as_millis());
    }
}
