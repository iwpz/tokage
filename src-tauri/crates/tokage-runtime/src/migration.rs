//! Engine identity is immutable. Legacy IDs are history; importing a plan uses a fresh run ID.
use crate::{repository::SqliteRepository, *};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::{Path, PathBuf};
fn error(e: impl std::fmt::Display) -> Error {
    Error::Repository(e.to_string())
}
pub(crate) fn admit_new(repo: &SqliteRepository, request: &Request) -> Result<()> {
    let Command::CreateRun { config, .. } = &request.command else {
        return Err(Error::Transition);
    };
    for kind in ["cli-v1", "app-pipeline-v1"] {
        if repo.history(kind, &request.run_id)?.is_some() {
            return Err(Error::Invalid(
                "Historical IDs cannot execute; create a fresh run and import an ordinary plan"
                    .into(),
            ));
        }
    }
    let data = repo.directory().parent().ok_or(Error::Transition)?;
    let id = &request.run_id;
    if id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && data.join("runs").join(id).join("run.json").exists()
    {
        return Err(Error::Invalid(
            "Legacy CLI ID is read-only; use a fresh run ID".into(),
        ));
    }
    let mut paths = vec![PathBuf::from(&config.workspace)];
    let database = data.join("tokage.db");
    if database.exists() {
        let conn = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(error)?;
        let known:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='main_sessions')",[],|r|r.get(0)).map_err(error)?;
        if !known {
            return Err(Error::Schema);
        }
        let approved: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE main_session_id=?1)",
                [id],
                |r| r.get(0),
            )
            .map_err(error)?;
        if approved {
            return Err(Error::Invalid("Legacy App run ID is reserved; finish it in its original engine and use a fresh shared run ID".into()));
        }
        let row:Option<(String,Option<String>)>=conn.query_row("SELECT p.local_path,s.worktree_json FROM main_sessions s JOIN projects p ON p.id=s.project_id WHERE s.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(error)?;
        if let Some((path, worktree)) = row {
            paths.push(path.into());
            if let Some(text) = worktree {
                let value: serde_json::Value = serde_json::from_str(&text).map_err(error)?;
                if let Some(cwd) = value["cwd"].as_str() {
                    paths.push(cwd.into());
                }
            }
        }
    }
    // Mirrors only the legacy filename encoding; never invokes its recovery/writer functions.
    let clean: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    for cwd in paths {
        if legacy_file(&cwd, &clean).exists() {
            return Err(Error::Invalid(
                "Legacy pipeline IDs cannot switch engines; import the plan into a fresh run"
                    .into(),
            ));
        }
    }
    Ok(())
}
fn legacy_file(cwd: &Path, id: &str) -> PathBuf {
    cwd.join(".tokage/execution").join(format!("{id}.json"))
}
