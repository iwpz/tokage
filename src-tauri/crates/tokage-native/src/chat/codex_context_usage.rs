//! Reconcile a completed Codex exec with the local rollout for its native thread.

use super::quota::{format_unix_to_utc_iso, parse_utc_iso_to_unix};
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokage_core::event::{EventKind, NativeTokenTotal};

const MAX_ROLLOUT_BYTES: u64 = 32 * 1024 * 1024;
const DAY_SECONDS: i64 = 86_400;

pub(super) struct ExecContextUsage {
    sessions: PathBuf,
    cwd: PathBuf,
    started_ms: i64,
    thread: Option<String>,
    total: Option<NativeTokenTotal>,
    conflicting: bool,
}

impl ExecContextUsage {
    pub(super) fn new(cmd: &std::process::Command, cwd: &Path) -> Option<Self> {
        let env = |name: &str| {
            cmd.get_envs()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .and_then(|(_, value)| value)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let home = env("CODEX_HOME").or_else(|| {
            env(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(|home| home.join(".codex"))
        })?;
        Some(Self {
            sessions: cwd.join(home).join("sessions"),
            cwd: cwd.into(),
            started_ms: now_ms()?,
            thread: None,
            total: None,
            conflicting: false,
        })
    }

    pub(super) fn observe(&mut self, event: &EventKind) {
        match event {
            EventKind::Thread { id } => {
                self.conflicting |= self.thread.as_ref().is_some_and(|old| old != id);
                self.thread = Some(id.clone());
            }
            EventKind::UsageAccounting { snapshot } => {
                self.conflicting |= self.total.is_some() && self.total != snapshot.total;
                self.total = snapshot.total.clone();
            }
            _ => {}
        }
    }

    pub(super) fn resolve(&self, finished_ms: i64) -> Option<u64> {
        if self.conflicting || finished_ms < self.started_ms {
            return None;
        }
        let thread = self.thread.as_deref()?;
        let path = find_rollout(&self.sessions, thread, self.started_ms)?;
        let cwd = self.cwd.canonicalize().ok()?;
        let file = File::open(path).ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() || metadata.len() > MAX_ROLLOUT_BYTES {
            return None;
        }
        read_usage(
            BufReader::new(file.take(MAX_ROLLOUT_BYTES + 1)),
            thread,
            &cwd,
            self.started_ms..=finished_ms,
            self.total.as_ref()?,
        )
    }
}

pub(super) fn now_ms() -> Option<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}

fn find_rollout(sessions: &Path, thread: &str, started_ms: i64) -> Option<PathBuf> {
    if !thread.split('-').map(str::len).eq([8, 4, 4, 4, 12])
        || !thread.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit())
    {
        return None;
    }
    // UUIDv7 encodes the thread's creation time, including for a resumed exec.
    // Codex groups rollout files by local calendar date, so check adjacent UTC dates.
    let created_ms = if thread.as_bytes()[14] == b'7' {
        i64::from_str_radix(&format!("{}{}", &thread[..8], &thread[9..13]), 16).ok()?
    } else {
        started_ms
    };
    let mut found = None;
    let suffix = format!("-{thread}.jsonl");
    for offset in [-DAY_SECONDS, 0, DAY_SECONDS] {
        let date = format_unix_to_utc_iso(created_ms / 1000 + offset);
        let day = sessions.join(date.get(..10)?.replace('-', "/"));
        let Ok(entries) = day.read_dir() else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name
                .to_str()
                .is_some_and(|s| s.starts_with("rollout-") && s.ends_with(&suffix))
                || !entry.file_type().is_ok_and(|kind| kind.is_file())
            {
                continue;
            }
            if found.replace(entry.path()).is_some() {
                return None;
            }
        }
    }
    found
}

fn timestamp_ms(utc: &str) -> Option<i64> {
    let whole = utc.get(..19)?;
    if !whole.bytes().enumerate().all(|(i, b)| match i {
        4 | 7 => b == b'-',
        10 => b == b'T',
        13 | 16 => b == b':',
        _ => b.is_ascii_digit(),
    }) {
        return None;
    }
    for (field, valid) in [
        (&whole[5..7], 1..=12),
        (&whole[8..10], 1..=31),
        (&whole[11..13], 0..=23),
        (&whole[14..16], 0..=59),
        (&whole[17..19], 0..=59),
    ] {
        if !valid.contains(&field.parse::<u8>().ok()?) {
            return None;
        }
    }
    let (seconds, ms) = if let Some((whole, fraction)) = utc.strip_suffix('Z')?.split_once('.') {
        if whole.len() != 19 || fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let ms = fraction
            .bytes()
            .chain(std::iter::repeat(b'0'))
            .take(3)
            .fold(0, |n, b| n * 10 + i64::from(b - b'0'));
        (parse_utc_iso_to_unix(&format!("{whole}Z")).ok()?, ms)
    } else {
        (parse_utc_iso_to_unix(utc).ok()?, 0)
    };
    seconds.checked_mul(1000)?.checked_add(ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    const THREAD: &str = "01a1f2bf-5f43-7d50-a189-1f28a7e089ff";
    const OTHER: &str = "01a1f2bf-5f43-7b12-a41f-54a5f3fc998b";

    fn row(ts: &str, kind: &str, payload: Value) -> Value {
        json!({"timestamp":format!("2026-11-19T07:40:{ts}Z"),"type":kind,"payload":payload})
    }

    fn turn(id: &str, start: &str, end: &str, cwd: &Path, input: u64, total: u64) -> Vec<Value> {
        vec![
            row(
                start,
                "event_msg",
                json!({"type":"task_started","turn_id":id}),
            ),
            row(start, "turn_context", json!({"turn_id":id,"cwd":cwd})),
            row(
                start,
                "event_msg",
                json!({"type":"token_count","info":{
                    "last_token_usage":{"input_tokens":input},
                    "total_token_usage":{"input_tokens":total,"cached_input_tokens":0,"output_tokens":0}
                }}),
            ),
            row(
                end,
                "event_msg",
                json!({"type":"task_complete","turn_id":id}),
            ),
        ]
    }

    fn extract(cwd: &Path, mut rows: Vec<Value>, exec_input: u64) -> Option<u64> {
        rows.insert(0, json!({"type":"session_meta","payload":{"id":THREAD}}));
        let data = rows
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let start = timestamp_ms("2026-11-19T07:40:10.500Z").unwrap();
        let end = timestamp_ms("2026-11-19T07:40:40.500Z").unwrap();
        let receipt = NativeTokenTotal {
            input_including_cache: exec_input,
            cache_read: 0,
            cache_write: 0,
            output: 0,
        };
        read_usage(data.as_bytes(), THREAD, cwd, start..=end, &receipt)
    }

    #[test]
    fn extracts_the_last_input_instead_of_cumulative_exec_usage() {
        let work = tempfile::tempdir().unwrap();
        let cwd = work.path().canonicalize().unwrap();
        let mut rows = turn("current", "11", "30", &cwd, 21_146, 90_000);
        rows.insert(2, row("12", "event_msg", json!({"type":"token_count","info":{
            "last_token_usage":{"input_tokens":19_000},
            "total_token_usage":{"input_tokens":19_000,"cached_input_tokens":0,"output_tokens":0}
        }})));
        assert_eq!(extract(&cwd, rows.clone(), 90_000), Some(21_146));
        assert_eq!(extract(&cwd, rows, 89_999), None);
    }

    #[test]
    fn a_resumed_exec_requires_current_completion_and_matches_its_counter_delta() {
        let work = tempfile::tempdir().unwrap();
        let cwd = work.path().canonicalize().unwrap();
        let mut rows = turn("previous", "01", "10.490", &cwd, 10_000, 20_000);
        assert_eq!(extract(&cwd, rows.clone(), 20_000), None);
        rows.extend(turn("current", "11", "30", &cwd, 21_146, 90_000));
        assert_eq!(extract(&cwd, rows, 70_000), Some(21_146));
    }

    #[test]
    fn refuses_missing_ambiguous_or_wrong_workspace_evidence() {
        let work = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let cwd = work.path().canonicalize().unwrap();
        let mut rows = turn("current", "11", "30", elsewhere.path(), 25, 100);
        assert_eq!(extract(&cwd, rows, 100), None);
        rows = turn("current", "11", "30", &cwd, 25, 100);
        rows.pop();
        assert_eq!(extract(&cwd, rows, 100), None);
        rows = turn("current", "11", "30", &cwd, 25, 100);
        rows.extend(turn("other", "31", "32", &cwd, 25, 100));
        assert_eq!(extract(&cwd, rows, 100), None);
        rows = turn("current", "11", "30", &cwd, 0, 0);
        assert_eq!(extract(&cwd, rows, 0), Some(0));
    }

    #[test]
    fn uses_the_threads_day_and_requires_an_unambiguous_rollout_filename() {
        let root = tempfile::tempdir().unwrap();
        let day = root.path().join("2026/11/19");
        fs::create_dir_all(&day).unwrap();
        let path = day.join(format!("rollout-created-{THREAD}.jsonl"));
        fs::write(&path, "").unwrap();
        fs::write(day.join(format!("rollout-other-{OTHER}.jsonl")), "").unwrap();
        let resumed = timestamp_ms("2026-11-20T18:00:00Z").unwrap();
        assert_eq!(find_rollout(root.path(), THREAD, resumed), Some(path));
        fs::write(day.join(format!("rollout-copy-{THREAD}.jsonl")), "").unwrap();
        assert_eq!(find_rollout(root.path(), THREAD, resumed), None);
        assert_eq!(find_rollout(root.path(), "../../outside", resumed), None);
    }
}

struct TurnUsage {
    id: String,
    in_window: bool,
    matching_cwd: bool,
    context: Option<u64>,
    total: Option<NativeTokenTotal>,
}

fn token_total(value: &Value) -> Option<NativeTokenTotal> {
    Some(NativeTokenTotal {
        input_including_cache: value["input_tokens"].as_u64()?,
        cache_read: value["cached_input_tokens"].as_u64()?,
        cache_write: value
            .get("cache_write_input_tokens")
            .map(Value::as_u64)
            .unwrap_or(Some(0))?,
        output: value["output_tokens"].as_u64()?,
    })
}

fn usage_delta(total: &NativeTokenTotal, before: &NativeTokenTotal) -> Option<NativeTokenTotal> {
    Some(NativeTokenTotal {
        input_including_cache: total
            .input_including_cache
            .checked_sub(before.input_including_cache)?,
        cache_read: total.cache_read.checked_sub(before.cache_read)?,
        cache_write: total.cache_write.checked_sub(before.cache_write)?,
        output: total.output.checked_sub(before.output)?,
    })
}

fn read_usage(
    reader: impl BufRead,
    thread: &str,
    cwd: &Path,
    window: std::ops::RangeInclusive<i64>,
    receipt: &NativeTokenTotal,
) -> Option<u64> {
    let mut lines = reader.lines();
    let meta: Value = serde_json::from_str(&lines.next()?.ok()?).ok()?;
    let id = meta["payload"]["id"].as_str();
    let session_id = meta["payload"]["session_id"].as_str();
    if meta["type"] != "session_meta"
        || id.or(session_id) != Some(thread)
        || session_id.is_some_and(|s| s != thread)
    {
        return None;
    }
    let mut previous_total = None;
    let mut active: Option<TurnUsage> = None;
    let mut completed = None;
    for line in lines {
        let row: Value = serde_json::from_str(&line.ok()?).ok()?;
        let p = &row["payload"];
        let in_window = || {
            row["timestamp"]
                .as_str()
                .and_then(timestamp_ms)
                .is_some_and(|ms| window.contains(&ms))
        };
        match (row["type"].as_str(), p["type"].as_str()) {
            (Some("event_msg"), Some("task_started" | "turn_started")) => {
                if active.as_ref().is_some_and(|turn| turn.in_window) {
                    return None;
                }
                active = Some(TurnUsage {
                    id: p["turn_id"].as_str()?.to_owned(),
                    in_window: in_window(),
                    matching_cwd: false,
                    context: None,
                    total: None,
                });
            }
            (Some("turn_context"), _) => {
                if let Some(turn) = active
                    .as_mut()
                    .filter(|turn| turn.in_window && p["turn_id"] == turn.id)
                {
                    turn.matching_cwd = p["cwd"]
                        .as_str()
                        .and_then(|path| Path::new(path).canonicalize().ok())
                        .is_some_and(|path| path == cwd);
                }
            }
            (Some("event_msg"), Some("token_count")) if p["info"].is_object() => {
                if let Some(turn) = active.as_mut() {
                    if turn.in_window && !in_window() {
                        return None;
                    }
                    turn.context = p["info"]["last_token_usage"]["input_tokens"].as_u64();
                    turn.total = token_total(&p["info"]["total_token_usage"]);
                }
            }
            (Some("event_msg"), Some("task_complete" | "turn_complete")) => {
                let Some(turn) = active.take().filter(|turn| p["turn_id"] == turn.id) else {
                    continue;
                };
                if !turn.in_window {
                    previous_total = turn.total;
                    continue;
                }
                // A later or ambiguous completion is not evidence for this exec.
                if completed.is_some() || !in_window() || !turn.matching_cwd {
                    return None;
                }
                let total = turn.total?;
                let matches = &total == receipt
                    || previous_total
                        .as_ref()
                        .and_then(|before| usage_delta(&total, before))
                        .as_ref()
                        == Some(receipt);
                let context = turn.context?;
                if !matches || context > receipt.input_including_cache {
                    return None;
                }
                completed = Some(context);
            }
            _ => {}
        }
    }
    if active.is_some_and(|turn| turn.in_window) {
        return None;
    }
    completed
}
