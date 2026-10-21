//! Durable CLI-owned state. No desktop Store or database access.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokage_core::{planning::Plan, ProviderId};
use tokage_native::{
    file_lease::FileLease,
    git::{scope::ScopeSnapshot, CheckoutIdentity, Worktree},
};

pub fn hash<T: Serialize>(value: &T) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}
pub fn nonce() -> String {
    format!(
        "r-{:x}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    Draft,
    Planning,
    AwaitingApproval,
    Approved,
    Executing,
    Blocked,
    Cancelled,
    AwaitingMerge,
    Merging,
    Delivered,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub plan_sha: String,
    pub confirmed_commands: Vec<String>,
    pub allow_writes: bool,
    pub trust_workspace: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub schema_version: u32,
    pub id: String,
    pub repo: PathBuf,
    pub target: CheckoutIdentity,
    pub provider: ProviderId,
    pub model: String,
    pub provider_path: Option<String>,
    pub requirement: String,
    pub status: Status,
    pub plan: Option<Plan>,
    pub approval: Option<Approval>,
    pub integration: Option<Worktree>,
    pub completed_tasks: BTreeMap<String, String>,
    pub task_reports: BTreeMap<String, String>,
    pub sequence: u32,
    pub error: Option<String>,
    pub accepted: Option<ScopeSnapshot>,
    pub review: Option<serde_json::Value>,
    pub co_author: bool,
}
impl Run {
    /// Binds model, requirement, checkout and the complete normalized plan to consent.
    pub fn plan_sha(&self) -> Result<String, String> {
        hash(
            &serde_json::json!({"schema":self.schema_version,"repo":self.repo,"target":self.target,"provider":self.provider,"model":self.model,"providerPath":self.provider_path,"requirement":self.requirement,"plan":self.plan,"coAuthor":self.co_author}),
        )
    }
    pub fn verify_approval(&self) -> Result<&Approval, String> {
        let a = self.approval.as_ref().ok_or("Plan has not been approved")?;
        if a.plan_sha != self.plan_sha()? || !a.allow_writes || !a.trust_workspace {
            return Err("Approval is missing or stale".into());
        }
        Ok(a)
    }
}

pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn new(path: Option<PathBuf>) -> Result<Self, String> {
        let root = match path {
            Some(p) => p,
            None => {
                #[cfg(windows)]
                let p = std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .ok_or("LOCALAPPDATA unavailable; specify --data-dir")?
                    .join("TokageCLI");
                #[cfg(not(windows))]
                let p = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .ok_or("HOME unavailable; specify --data-dir")?
                    .join(".local/share/tokage-cli");
                p
            }
        };
        Ok(Self {
            root: if root.is_absolute() {
                root
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(root)
            },
        })
    }
    pub fn run_dir(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty()
            || id.len() > 64
            || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err("Invalid run ID".into());
        }
        Ok(self.root.join("runs").join(id))
    }
    pub fn initialize(&self, id: &str) -> Result<PathBuf, String> {
        let dir = self.run_dir(id)?;
        private_dir(&self.root.join("runs"))?;
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        Ok(dir)
    }
    pub fn load(&self, id: &str) -> Result<Run, String> {
        let p = self.run_dir(id)?.join("run.json");
        if fs::symlink_metadata(&p)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Refusing symlinked run state".into());
        }
        let run: Run = serde_json::from_slice(&fs::read(p).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if run.id != id || run.schema_version != 1 {
            return Err("Unknown or mismatched run state".into());
        }
        Ok(run)
    }
    pub fn lease(&self, id: &str) -> Result<FileLease, String> {
        let dir = self.run_dir(id)?;
        if !dir.is_dir() {
            return Err("Run not found".into());
        }
        FileLease::acquire(&dir.join("owner.lock"))
            .map_err(|e| format!("Run is owned by another process or cannot be locked: {e}"))
    }
    pub fn save(&self, run: &Run) -> Result<(), String> {
        atomic_json(&self.run_dir(&run.id)?.join("run.json"), run)
    }
    pub fn event(&self, id: &str, value: &serde_json::Value) -> Result<(), String> {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.run_dir(id)?.join("events.jsonl"))
            .map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut f, value).map_err(|e| e.to_string())?;
        f.write_all(b"\n")
            .and_then(|_| f.sync_data())
            .map_err(|e| e.to_string())
    }
    pub fn cancelled(&self, id: &str) -> bool {
        self.run_dir(id)
            .map(|p| p.join("cancel.json").exists())
            .unwrap_or(true)
    }
}
fn private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let temp = path.with_extension(format!("{}.tmp", nonce()));
    let result = (|| {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&temp).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(&mut file, value).map_err(|e| e.to_string())?;
        file.write_all(b"\n")
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            fs::File::open(path.parent().ok_or("Missing state parent")?)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
