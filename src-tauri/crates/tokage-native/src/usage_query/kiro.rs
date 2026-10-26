//! Kiro social account usage: fixed official management endpoint, CLI-managed refresh fallback.
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
// Account/profile identity is independent of routine grant rotation.
fn local_source(env: &ShellEnv) -> Option<(Value, PathBuf)> {
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
    Some((v, db))
}
pub(crate) fn resolve(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<Identity> {
    let (v, db) = local_source(env)?;
    source_identity(&v, exe, &db, salt)
}
pub(crate) fn observe(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<identity::Observation> {
    let (v, db)=local_source(env)?;
    Some(identity::Observation { claude_lifecycle: None, identity:source_identity(&v,exe,&db,salt)?, source_login:None,
        credential_revision:Some(identity::opaque_key(salt,&[b"kiro-local-credential-revision", v["refresh_token"].as_str()?.as_bytes(), v["access_token"].as_str().unwrap_or("").as_bytes()])) })
}
/// Configured CLI-owned social login evidence; not a claim that the remote endpoint is reachable.
pub(crate) fn login_label(env: &ShellEnv) -> Option<&'static str> {
    let (v, db) = local_source(env)?;
    source_identity(&v, Path::new("kiro-cli"), &db, b"local-login-evidence")?;
    match v["provider"].as_str()? {
        "google" => Some("Google account"),
        "github" => Some("GitHub account"),
        _ => None,
    }
}
fn source_identity(v: &Value, exe: &Path, db: &Path, salt: &[u8]) -> Option<Identity> {
    let provider = v["provider"]
        .as_str()
        .filter(|p| matches!(*p, "google" | "github"))?;
    let _grant = v["refresh_token"].as_str().filter(|s| !s.is_empty())?;
    let profile = v["profile_arn"].as_str().filter(|s| !s.is_empty())?;
    super::http::UsageHttpTarget::kiro(profile).ok()?;
    Some(Identity {
        key: identity::opaque_key(
            salt,
            &[
                b"kiro-social-cli-store",
                exe.as_os_str().as_encoded_bytes(),
                db.as_os_str().as_encoded_bytes(),
                provider.as_bytes(),
                profile.as_bytes(),
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

/// Fixed vendor resource, current CLI-owned credentials, and no model/catalog bootstrap.
/// On rejected bearer credentials only, let the official management CLI own any refresh.
/// The service checks the stable profile before/after; CLI grant rotation preserves that binding.
pub(crate) async fn query(
    permit: &crate::online_usage::QueryPermit,
    exe: &Path,
    env: &ShellEnv,
    proxy: &PreparedTurnProxy,
) -> Result<QueryOutput, QueryFailure> {
    permit.publish(|| ()).map_err(|_| failure(QueryError::Cancelled))?;
    let (v, _) = local_source(env).ok_or_else(|| failure(QueryError::Identity))?;
    let profile = v["profile_arn"]
        .as_str()
        .ok_or_else(|| failure(QueryError::Identity))?;
    let target = super::http::UsageHttpTarget::kiro(profile)?;
    let response = match v["access_token"].as_str().filter(|s| !s.is_empty()) {
        Some(token) => match super::http::UsageCredential::bearer(token.into()) {
            Ok(credential) => super::http::get(permit,target,credential,env,proxy).await,
            Err(error) => Err(error),
        },
        None => Err(failure(QueryError::Authentication)),
    };
    match response {
        Ok(value) => parse_account(&value),
        Err(error) if error.category == QueryError::Authentication => {
            match permit.run(async { Ok(refresh_query(permit,exe,env,proxy,&v).await) }).await.map_err(|_| failure(QueryError::Cancelled))? {
                // A failed management launch/sign-in panel must not hide the known auth rejection.
                Err(e) if matches!(e.category, QueryError::Network | QueryError::InvalidData) => {
                    Err(error)
                }
                result => result,
            }
        }
        Err(error) => Err(error),
    }
}
/// The legacy UI may bootstrap catalogs long after the CLI has refreshed its token.
/// Once that official write is visible for this same profile, cancel the slow UI and
/// retry the fixed resource once. Tokage never obtains/rotates grants itself.
async fn refresh_query(permit:&crate::online_usage::QueryPermit,exe:&Path,env:&ShellEnv,proxy:&PreparedTurnProxy,previous:&Value)->Result<QueryOutput,QueryFailure> {
    let updated={
        let mut management=Box::pin(legacy_query(exe,env,proxy));
        let mut poll=tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tokio::select! {
                result=&mut management => return result,
                _=poll.tick()=>{
                    let Some((current,_))=local_source(env) else { continue; };
                    if current["provider"]!=previous["provider"] || current["profile_arn"]!=previous["profile_arn"] { return Err(failure(QueryError::Identity)); }
                    if current["access_token"].as_str().is_some_and(|s|!s.is_empty()) && current["access_token"]!=previous["access_token"] { break current; }
                }
            }
        }
    }; // Dropping management here kills its process group before another request.
    let target=super::http::UsageHttpTarget::kiro(updated["profile_arn"].as_str().ok_or_else(||failure(QueryError::Identity))?)?;
    let credential=super::http::UsageCredential::bearer(updated["access_token"].as_str().ok_or_else(||failure(QueryError::Authentication))?.into())?;
    parse_account(&super::http::get(permit,target,credential,env,proxy).await?)
}
fn number(v: &Value, precise: &str, fallback: &str) -> Result<String, QueryFailure> {
    let v = if v[precise].is_null() {
        &v[fallback]
    } else {
        &v[precise]
    };
    let text = v
        .as_number()
        .map(ToString::to_string)
        .ok_or_else(|| failure(QueryError::InvalidData))?;
    if !decimal(&text) {
        return Err(failure(QueryError::InvalidData));
    }
    Ok(text)
}
fn timestamp(v: &Value) -> Result<Option<i64>, QueryFailure> {
    if v.is_null() {
        return Ok(None);
    }
    let value = v
        .as_f64()
        .filter(|n| n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= 253402300799.0)
        .ok_or_else(|| failure(QueryError::InvalidData))?;
    Ok(Some(value as i64))
}
fn label(v: &Value) -> Result<&str, QueryFailure> {
    v.as_str()
        .filter(|s| !s.is_empty() && s.len() <= 100 && !s.chars().any(char::is_control))
        .ok_or_else(|| failure(QueryError::InvalidData))
}
/// Credits are vendor estimates. Each pool stays separate; API currency/charges are not balances.
/// No user identifier/email/profile or arbitrary response text enters persisted account snapshots.
pub fn parse_account(v: &Value) -> Result<QueryOutput, QueryFailure> {
    let plan = label(&v["subscriptionInfo"]["subscriptionTitle"])?;
    let rows = v["usageBreakdownList"]
        .as_array()
        .filter(|r| !r.is_empty() && r.len() <= 16)
        .ok_or_else(|| failure(QueryError::InvalidData))?;
    let mut out = QueryOutput {
        available: None,
        source: QuerySource::Http,
        windows: vec![],
        quantities: vec![],
        display: None,
    };
    let mut credits = rows.iter().filter(|r| r["resourceType"] == "CREDIT");
    let row = credits
        .next()
        .ok_or_else(|| failure(QueryError::InvalidData))?;
    // Multiple CREDIT dimensions require an explicit mapping, never guess a combined limit.
    if credits.next().is_some()
        || !row["addOnMetadata"].is_null()
        || !row["dimensionType"].is_null()
    {
        return Err(failure(QueryError::InvalidData));
    }
    fn pool(
        out: &mut QueryOutput,
        v: &Value,
        id: &str,
        scope: &str,
        period: String,
        reset: Option<i64>,
    ) -> Result<(), QueryFailure> {
        let used = number(v, "currentUsageWithPrecision", "currentUsage")?;
        let limit = number(v, "usageLimitWithPrecision", "usageLimit")?;
        let u = used
            .parse::<f64>()
            .map_err(|_| failure(QueryError::InvalidData))?;
        let l = limit
            .parse::<f64>()
            .map_err(|_| failure(QueryError::InvalidData))?;
        if !u.is_finite() || !l.is_finite() {
            return Err(failure(QueryError::InvalidData));
        }
        out.windows.push(QuotaWindow {
            id: id.into(),
            scope: scope.into(),
            period,
            window_minutes: None,
            used_percent: (l > 0.0).then(|| (u / l * 100.0).min(100.0)),
            resets_at: reset,
        });
        out.quantities.push(Quantity {
            id: id.into(),
            unit: "credits".into(),
            currency: None,
            used: Some(used),
            limit: Some(limit),
            remaining: None,
        });
        Ok(())
    }
    let reset = if row["nextDateReset"].is_null() {
        timestamp(&v["nextDateReset"])?
    } else {
        timestamp(&row["nextDateReset"])?
    };
    pool(
        &mut out,
        row,
        "estimated-plan-credits",
        "plan:estimated",
        format!("Estimated · {plan}"),
        reset,
    )?;
    if !row["freeTrialInfo"].is_null() {
        let trial = &row["freeTrialInfo"];
        let status = label(&trial["freeTrialStatus"])?;
        pool(
            &mut out,
            trial,
            "estimated-trial-credits",
            "trial:estimated",
            format!("Estimated · Free trial · {status}"),
            timestamp(&trial["freeTrialExpiry"])?,
        )?;
    }
    if !row["bonuses"].is_null() {
        let bonuses = row["bonuses"]
            .as_array()
            .filter(|r| r.len() <= 32)
            .ok_or_else(|| failure(QueryError::InvalidData))?;
        for (i, bonus) in bonuses.iter().enumerate() {
            let name = label(&bonus["displayName"])?;
            let status = label(&bonus["status"])?;
            pool(
                &mut out,
                bonus,
                &format!("estimated-bonus-{i}"),
                "bonus:estimated",
                format!("Estimated · {name} · {status}"),
                timestamp(&bonus["expiresAt"])?,
            )?;
        }
    }
    if !row["currentOveragesWithPrecision"].is_null() || !row["currentOverages"].is_null() {
        let used = number(row, "currentOveragesWithPrecision", "currentOverages")?;
        if used.parse::<f64>().is_ok_and(|u| u > 0.0) {
            out.windows.push(QuotaWindow {
                id: "estimated-overage-credits".into(),
                scope: "overage:estimated".into(),
                period: "Estimated · Overage credits".into(),
                window_minutes: None,
                used_percent: None,
                resets_at: reset,
            });
            out.quantities.push(Quantity {
                id: "estimated-overage-credits".into(),
                unit: "credits".into(),
                currency: None,
                used: Some(used),
                limit: None,
                remaining: None,
            });
        }
    }
    Ok(out)
}

/// A fresh normal workspace/profile prevents user agents, resources and MCP startup.
/// CLI-owned HOME and authentication remain unchanged.
async fn legacy_query(
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
    fn account() -> Value {
        serde_json::json!({
        "subscriptionInfo":{"subscriptionTitle":"KIRO PRO+"},"nextDateReset":1793491200.0,
        "usageBreakdownList":[{"resourceType":"CREDIT","unit":"INVOCATIONS","currency":"USD",
            "currentUsage":123,"currentUsageWithPrecision":123.45,"usageLimit":2000,"usageLimitWithPrecision":2000.0,
            "currentOveragesWithPrecision":0.0,"bonuses":[],"freeTrialInfo":null}],
        "userInfo":{"email":"must-not-persist@example.invalid","userId":"private-user"} })
    }
    #[test]
    fn account_preserves_precision_reset_and_separate_pools_without_money_or_identity() {
        let mut v = account();
        v["usageBreakdownList"][0]["freeTrialInfo"] = serde_json::json!({"freeTrialStatus":"ACTIVE","freeTrialExpiry":1793491200.0,"currentUsageWithPrecision":1.5,"usageLimitWithPrecision":50.0});
        v["usageBreakdownList"][0]["bonuses"] = serde_json::json!([{"displayName":"Bonus","status":"ACTIVE","expiresAt":1793491200,"currentUsage":2,"usageLimit":10}]);
        v["usageBreakdownList"][0]["currentOveragesWithPrecision"] = serde_json::json!(0.5);
        let q = parse_account(&v).unwrap();
        assert!(matches!(q.source, QuerySource::Http));
        assert_eq!(q.quantities.len(), 4);
        assert_eq!(q.quantities[0].used.as_deref(), Some("123.45"));
        assert_eq!(q.windows[0].resets_at, Some(1793491200));
        assert_eq!(q.windows[0].scope, "plan:estimated");
        assert_eq!(q.windows[1].scope, "trial:estimated");
        assert!(q
            .quantities
            .iter()
            .all(|q| q.unit == "credits" && q.currency.is_none() && q.remaining.is_none()));
        assert!(q.quantities[3].limit.is_none() && q.windows[3].used_percent.is_none());
        assert!(q.windows.iter().all(|w| w.scope != "account"));
    }
    #[test]
    fn account_rejects_ambiguous_malformed_and_negative_data_without_zero_substitution() {
        for (key, value) in [
            ("currentUsageWithPrecision", serde_json::json!(-1)),
            ("usageLimitWithPrecision", serde_json::json!("2000")),
            ("nextDateReset", serde_json::json!(1793491200.5)),
            ("dimensionType", serde_json::json!("UNKNOWN")),
        ] {
            let mut v = account();
            v["usageBreakdownList"][0][key] = value;
            assert!(parse_account(&v).is_err(), "{key}");
        }
        let mut v = account();
        let duplicate = v["usageBreakdownList"][0].clone();
        v["usageBreakdownList"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(parse_account(&v).is_err());
        let mut v = account();
        v["usageBreakdownList"][0]["currentUsageWithPrecision"] = serde_json::json!(2001.25);
        let q = parse_account(&v).unwrap();
        assert_eq!(q.quantities[0].used.as_deref(), Some("2001.25"));
        assert_eq!(q.windows[0].used_percent, Some(100.0));
        v["usageBreakdownList"][0]["usageLimitWithPrecision"] = serde_json::json!(0);
        assert!(parse_account(&v).unwrap().windows[0].used_percent.is_none());
    }
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
            format!("{PANEL}\n{PANEL}"),
        ] {
            assert!(parse(&s).is_err());
        }
    }
    #[test]
    fn grant_rotation_preserves_profile_identity_but_backend_changes_invalidate_it() {
        let mut v = serde_json::json!({"provider":"google","profile_arn":"arn:aws:codewhisperer:us-east-1:123456789012:profile/synthetic","refresh_token":"synthetic-grant"});
        let key = source_identity(&v, Path::new("/cli"), Path::new("/db"), b"salt");
        assert!(key.is_some());
        v["refresh_token"] = serde_json::json!("rotated");
        assert!(key == source_identity(&v, Path::new("/cli"), Path::new("/db"), b"salt"));
        v["profile_arn"] = serde_json::json!("arn:aws:codewhisperer:us-east-1:123456789012:profile/different");
        assert!(key != source_identity(&v, Path::new("/cli"), Path::new("/db"), b"salt"));
        v["provider"] = serde_json::json!("unknown");
        assert!(source_identity(&v, Path::new("/cli"), Path::new("/db"), b"salt").is_none());
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn direct_query_does_not_boot_chat_and_auth_failure_stays_typed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        #[cfg(target_os = "macos")]
        let db_path = dir
            .path()
            .join("Library/Application Support/kiro-cli/data.sqlite3");
        #[cfg(not(target_os = "macos"))]
        let db_path = dir.path().join(".local/share/kiro-cli/data.sqlite3");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let db = rusqlite::Connection::open(db_path).unwrap();
        db.execute("CREATE TABLE auth_kv (key TEXT,value TEXT)", [])
            .unwrap();
        db.execute("INSERT INTO auth_kv VALUES ('kirocli:social:token',?1)",[serde_json::json!({"provider":"google","profile_arn":"arn:aws:codewhisperer:us-east-1:123456789012:profile/synthetic","refresh_token":"synthetic","access_token":"synthetic"}).to_string()]).unwrap();
        let curl = dir.path().join("curl");
        std::fs::write(
            &curl,
            r#"#!/bin/sh
/bin/cat >/dev/null
base=${0%/*}
/bin/cat "$base/response"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
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
        let proxy = crate::proxy::prepare_turn_proxy(
            &Default::default(),
            tokage_core::ProviderId::KiroCli,
            &env,
        )
        .await
        .unwrap();
        let cli = dir.path().join("nonexistent-kiro");
        let payload = serde_json::json!({"subscriptionInfo":{"subscriptionTitle":"KIRO FREE"},"usageBreakdownList":[{"resourceType":"CREDIT","currentUsage":1,"usageLimit":50}]});
        std::fs::write(
            dir.path().join("response"),
            format!("HTTP/1.1 200 OK\r\n\r\n{payload}\n200"),
        )
        .unwrap();
        let q = query(&permission.permit().unwrap(), &cli, &env, &proxy)
            .await
            .unwrap();
        assert_eq!(q.quantities[0].used.as_deref(), Some("1"));
        for (status, category) in [
            (429, QueryError::RateLimited),
            (401, QueryError::Authentication),
        ] {
            std::fs::write(
                dir.path().join("response"),
                format!("HTTP/1.1 {status} Failure\r\n\r\n{{}}\n{status}"),
            )
            .unwrap();
            assert_eq!(
                query(&permission.permit().unwrap(), &cli, &env, &proxy)
                    .await
                    .err()
                    .unwrap()
                    .category,
                category
            );
        }
        // Missing access credentials still reach the official refresh-capable management path.
        let revision_before=observe(Path::new("/cli"),&env,b"salt").unwrap();
        db.execute("UPDATE auth_kv SET value=?1",[serde_json::json!({"provider":"google","profile_arn":"arn:aws:codewhisperer:us-east-1:123456789012:profile/synthetic","refresh_token":"rotated"}).to_string()]).unwrap();
        let revision_after=observe(Path::new("/cli"),&env,b"salt").unwrap();
        assert!(revision_before.identity==revision_after.identity);
        assert_ne!(revision_before.credential_revision,revision_after.credential_revision);
        // Absent CLI still returns typed auth rather than losing the original rejection.
        assert_eq!(query(&permission.permit().unwrap(), &cli, &env, &proxy).await.err().unwrap().category,QueryError::Authentication);
        let refresh_cli=dir.path().join("kiro-management");
        std::fs::write(&refresh_cli,"#!/bin/sh\n[ \"$1\" = chat ] && [ \"${10}\" = /usage ] || exit 71\nprintf 'Estimated Usage | resets on 2026-11-01 | KIRO PRO+\\nCredits (1 of 50 covered in plan)\\n2%%\\n'\n").unwrap();
        std::fs::set_permissions(&refresh_cli,std::fs::Permissions::from_mode(0o755)).unwrap();
        let refreshed=query(&permission.permit().unwrap(),&refresh_cli,&env,&proxy).await.unwrap();
        assert!(matches!(refreshed.source,QuerySource::Cli));
        assert_eq!(refreshed.quantities[0].used.as_deref(),Some("1"));
        std::fs::write(dir.path().join("response"),format!("HTTP/1.1 200 OK\r\n\r\n{payload}\n200")).unwrap();
        let slow_cli=dir.path().join("kiro-refresh-slow-ui");
        std::fs::write(&slow_cli,format!(r#"#!/usr/bin/python3
import sqlite3,json,time,os
from pathlib import Path
p=Path(os.environ['HOME'])/{relative:?}
c=sqlite3.connect(p)
v=json.loads(c.execute("SELECT value FROM auth_kv").fetchone()[0]);v['access_token']='synthetic-new-access'
c.execute("UPDATE auth_kv SET value=?",(json.dumps(v),));c.commit();c.close()
time.sleep(10)
print('Estimated Usage | resets on 2026-11-01 | KIRO PRO+')
print('Credits (1 of 50 covered in plan)');print('2%')
"#,relative=if cfg!(target_os="macos") { "Library/Application Support/kiro-cli/data.sqlite3" } else { ".local/share/kiro-cli/data.sqlite3" })).unwrap();
        std::fs::set_permissions(&slow_cli,std::fs::Permissions::from_mode(0o755)).unwrap();
        let started=std::time::Instant::now();
        let refreshed=query(&permission.permit().unwrap(),&slow_cli,&env,&proxy).await.unwrap();
        assert!(matches!(refreshed.source,QuerySource::Http));
        assert!(started.elapsed()<std::time::Duration::from_secs(5),"must not wait for legacy catalog bootstrap after CLI refresh");
        let old_permit = permission.permit().unwrap();
        permission.set(false).unwrap();
        assert_eq!(
            query(&old_permit, &cli, &env, &proxy)
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
