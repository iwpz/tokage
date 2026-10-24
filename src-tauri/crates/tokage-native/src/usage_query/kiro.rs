//! Kiro's fixed legacy management command. No ordinary prompt or project context.
use super::{
    contract::*,
    identity::{self, Identity},
};
use crate::{env::ShellEnv, proxy::PreparedTurnProxy};
use serde_json::Value;
use std::path::{Path, PathBuf};
fn failure(category: QueryError) -> QueryFailure {
    QueryFailure {
        category,
        retry_after: None,
    }
}

// Narrow, read-only CLI-owned store; unknown/multiple auth namespaces fail closed.
// A grant change intentionally invalidates history instead of guessing account equivalence.
pub(crate) fn resolve(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<Identity> {
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
    let count: i64 = conn
        .query_row("SELECT count(*) FROM auth_kv", [], |r| r.get(0))
        .ok()?;
    if count != 1 {
        return None;
    }
    let text:String=conn.query_row("SELECT value FROM auth_kv WHERE key = 'kirocli:social:token' AND length(value) <= 65536",[],|r|r.get(0)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    source_identity(&v, exe, &db, salt)
}
fn source_identity(v: &Value, exe: &Path, db: &Path, salt: &[u8]) -> Option<Identity> {
    let provider = v["provider"]
        .as_str()
        .filter(|p| matches!(*p, "google" | "github"))?;
    let grant = v["refresh_token"].as_str().filter(|s| !s.is_empty())?;
    let profile = v["profile_arn"].as_str().filter(|s| !s.is_empty())?;
    Some(Identity {
        key: identity::opaque_key(
            salt,
            &[
                b"kiro-social-cli-store",
                exe.as_os_str().as_encoded_bytes(),
                db.as_os_str().as_encoded_bytes(),
                provider.as_bytes(),
                profile.as_bytes(),
                grant.as_bytes(),
            ],
        ),
    })
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

/// A fresh normal workspace/profile prevents user agents, resources and MCP startup.
/// CLI-owned HOME and authentication remain unchanged.
pub(crate) async fn query(
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
    let (status, output) = tokio::time::timeout(std::time::Duration::from_secs(90), async {
        tokio::join!(child.wait(), read)
    })
    .await
    .map_err(|_| failure(QueryError::Timeout))?;
    if !status.map_err(|_| failure(QueryError::Network))?.success() {
        return Err(failure(QueryError::Network));
    }
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
    parse(&text)
}
#[cfg(test)]
mod tests {
    use super::*;
    const PANEL:&str="Estimated Usage | resets on 2026-11-01 | KIRO PRO+\nCredits (123.40 of 2000 covered in plan)\n████ 6.2%\nAdditional credits\nAdd-on credits available for purchase";
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
        ] {
            assert!(parse(&s).is_err());
        }
    }
    #[test]
    fn source_rotation_and_backend_changes_invalidate_identity() {
        let mut v = serde_json::json!({"provider":"google","profile_arn":"synthetic-profile","refresh_token":"synthetic-grant"});
        let key = source_identity(&v, Path::new("/cli"), Path::new("/db"), b"salt");
        assert!(key.is_some());
        v["refresh_token"] = serde_json::json!("rotated");
        assert!(key != source_identity(&v, Path::new("/cli"), Path::new("/db"), b"salt"));
        v["provider"] = serde_json::json!("unknown");
        assert!(source_identity(&v, Path::new("/cli"), Path::new("/db"), b"salt").is_none());
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;
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
                r#"{"provider":"google","profile_arn":"synthetic","refresh_token":"synthetic"}"#
            ],
        )
        .unwrap();
        let mut env = ShellEnv {
            vars: [("HOME".into(), dir.path().to_string_lossy().into_owned())].into(),
        };
        assert!(resolve(Path::new("/cli"), &env, b"salt").is_some());
        env.vars
            .insert("KIRO_API_KEY".into(), "synthetic-override".into());
        assert!(resolve(Path::new("/cli"), &env, b"salt").is_none());
        env.vars.remove("KIRO_API_KEY");
        db.execute("INSERT INTO auth_kv VALUES ('other-auth','{}')", [])
            .unwrap();
        assert!(resolve(Path::new("/cli"), &env, b"salt").is_none());
    }
    #[tokio::test]
    #[ignore = "installed Kiro built-in /usage diagnostic; no model prompt; not desktop acceptance"]
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
        let q = query(&exe, &env, &proxy)
            .await
            .map_err(|e| e.category)
            .expect("built-in usage");
        assert!(Some(before) == resolve(&exe, &env, b"diagnostic"));
        println!("PASS management query: {} scoped windows, {} credit quantities, {}ms; identity unchanged",q.windows.len(),q.quantities.len(),started.elapsed().as_millis());
    }
}
