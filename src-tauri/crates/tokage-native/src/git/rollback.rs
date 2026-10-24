//! Content backups live in the per-worktree Git directory, outside agent write scope.
//! Restoration requires a sealed end state; it never guesses from HEAD or status.
use super::{
    run,
    scope::{self, ScopeSnapshot},
};
use crate::env::ShellEnv;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Backup {
    scope: ScopeSnapshot,
    index: Vec<u8>,
    modes: BTreeMap<String, u32>,
}
fn key(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
async fn directory(env: &ShellEnv, dir: &Path, id: &str) -> Result<PathBuf, String> {
    let git = run(env, dir, &["rev-parse", "--absolute-git-dir"], &[], 10).await?;
    Ok(Path::new(git.trim()).join("tokage-rollback").join(key(id)))
}
fn read<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, String> {
    serde_json::from_slice(&fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?)
        .map_err(|e| e.to_string())
}
fn save<T: Serialize>(p: &Path, v: &T) -> Result<(), String> {
    let mut f = tempfile::NamedTempFile::new_in(p.parent().ok_or("Missing backup parent")?)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut f, v).map_err(|e| e.to_string())?;
    f.as_file().sync_all().map_err(|e| e.to_string())?;
    f.persist_noclobber(p).map_err(|e| e.to_string())?;
    sync_dir(p.parent().unwrap())
}
fn sync_dir(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn index_path(git_backup: &Path) -> PathBuf {
    git_backup.parent().unwrap().parent().unwrap().join("index")
}
fn bytes(p: &Path) -> Result<Vec<u8>, String> {
    fs::read(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn modes(snapshot: &ScopeSnapshot) -> Result<BTreeMap<String, u32>, String> {
    snapshot
        .files
        .iter()
        .filter(|(_, entry)| entry.disk.starts_with("file:"))
        .map(|(path, _)| {
            let meta = fs::symlink_metadata(Path::new(&snapshot.root).join(path))
                .map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err(format!("File type changed: {path}"));
            }
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode()
            };
            #[cfg(not(unix))]
            let mode = meta.permissions().readonly() as u32;
            Ok((path.clone(), mode))
        })
        .collect()
}

/// One Git process streams deduplicated raw index blobs; no per-file CLI launch cost.
async fn backup_objects(
    env: &ShellEnv,
    root: &Path,
    dest: &Path,
    expected: &ScopeSnapshot,
) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let objects: std::collections::BTreeSet<_> = expected
        .files
        .values()
        .filter(|s| !s.index.is_empty())
        .map(|s| s.index.split(' ').nth(1).ok_or("Malformed backup index"))
        .collect::<Result<_, _>>()?;
    if objects.is_empty() {
        return Ok(());
    }
    let input = objects.iter().map(|s| format!("{s}\n")).collect::<String>();
    let mut child = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .env_clear()
        .envs(&env.vars)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("Missing Git stdin")?;
    let writer = tokio::spawn(async move { stdin.write_all(input.as_bytes()).await });
    let mut stream = tokio::io::BufReader::new(child.stdout.take().ok_or("Missing Git stdout")?);
    let mut total = 0u64;
    for oid in objects {
        let mut header = String::new();
        stream
            .read_line(&mut header)
            .await
            .map_err(|e| e.to_string())?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != oid || fields[1] != "blob" {
            return Err(format!("Cannot back up staged object: {oid}"));
        }
        let size: u64 = fields[2]
            .parse()
            .map_err(|_| "Invalid staged object size")?;
        total = total
            .checked_add(size)
            .ok_or("Object backup size overflow")?;
        if total > 512 * 1024 * 1024 {
            return Err("Staged content backup exceeds 512 MiB".into());
        }
        let mut output = tokio::fs::File::create(dest.join(format!("object-{oid}")))
            .await
            .map_err(|e| e.to_string())?;
        let copied = tokio::io::copy(&mut (&mut stream).take(size), &mut output)
            .await
            .map_err(|e| e.to_string())?;
        output.sync_all().await.map_err(|e| e.to_string())?;
        if copied != size || stream.read_u8().await.map_err(|e| e.to_string())? != b'\n' {
            return Err("Truncated staged object backup".into());
        }
    }
    writer
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    if !child.wait().await.map_err(|e| e.to_string())?.success() {
        return Err("Git staged object backup failed".into());
    }
    Ok(())
}

/// The caller's scope read and the byte backup must describe exactly the same checkout.
pub async fn capture(
    env: &ShellEnv,
    dir: &Path,
    id: &str,
    expected: &ScopeSnapshot,
) -> Result<(), String> {
    if &scope::snapshot(env, dir).await? != expected {
        return Err("Backup request does not match verified checkout".into());
    }
    let dest = directory(env, dir, id).await?;
    if dest.exists() {
        return Err("Rollback identity already exists; refusing to replace original backup".into());
    }
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dest.parent().unwrap(), fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    sync_dir(dest.parent().unwrap().parent().unwrap())?;
    let tmp = tempfile::tempdir_in(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    let root = Path::new(&expected.root);
    let visible = run(
        env,
        root,
        &["diff", "--cached", "--ita-visible-in-index", "--raw", "-z"],
        &[],
        20,
    )
    .await?;
    let invisible = run(
        env,
        root,
        &[
            "diff",
            "--cached",
            "--ita-invisible-in-index",
            "--raw",
            "-z",
        ],
        &[],
        20,
    )
    .await?;
    if visible != invisible {
        return Err(
            "Intent-to-add index entries cannot be restored reliably; launch stopped".into(),
        );
    }
    let original_index = bytes(&index_path(&dest))?;
    let mut modes = BTreeMap::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        backup_objects(env, root, tmp.path(), expected),
    )
    .await
    .map_err(|_| "Staged content backup timed out")??;
    for (path, entry) in &expected.files {
        if entry.disk.starts_with("file:") {
            let source = root.join(path);
            let content = bytes(&source)?;
            // Validate each copied byte against the accepted SHA, not merely two surrounding reads.
            if !entry
                .disk
                .ends_with(&format!(":{:x}", Sha256::digest(&content)))
            {
                return Err(format!("File changed during backup: {path}"));
            }
            let target = tmp.path().join(key(path));
            let mut f = fs::File::create(&target).map_err(|e| e.to_string())?;
            f.write_all(&content)
                .and_then(|_| f.sync_all())
                .map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                modes.insert(
                    path.clone(),
                    fs::metadata(&source)
                        .map_err(|e| e.to_string())?
                        .permissions()
                        .mode(),
                );
            }
            #[cfg(not(unix))]
            {
                modes.insert(
                    path.clone(),
                    fs::metadata(&source)
                        .map_err(|e| e.to_string())?
                        .permissions()
                        .readonly() as u32,
                );
            }
        }
    }
    if &scope::snapshot(env, dir).await? != expected
        || modes != self::modes(expected)?
        || bytes(&index_path(&dest))? != original_index
    {
        return Err("Checkout changed during content backup".into());
    }
    save(
        &tmp.path().join("before.json"),
        &Backup {
            scope: expected.clone(),
            index: original_index,
            modes,
        },
    )?;
    fs::rename(tmp.path(), &dest).map_err(|e| e.to_string())?;
    sync_dir(dest.parent().unwrap())?;
    Ok(())
}

/// Called after CLI exit on success, failure and cancellation. Never refreshed by a restore click.
pub async fn seal(env: &ShellEnv, dir: &Path, id: &str) -> Result<(), String> {
    let dest = directory(env, dir, id).await?;
    if !dest.exists() {
        return Err("Original rollback content backup is missing".into());
    }
    let before: Backup = read(&dest.join("before.json"))?;
    let after = scope::snapshot(env, dir).await?;
    if after.root != before.scope.root
        || after.workdir != before.scope.workdir
        || after.head != before.scope.head
    {
        return Err("Repository identity changed; rollback cannot be sealed".into());
    }
    let after_modes = modes(&after)?;
    if scope::snapshot(env, dir).await? != after || modes(&after)? != after_modes {
        return Err("Checkout changed while sealing rollback evidence".into());
    }
    save(&dest.join("after-modes.json"), &after_modes)?;
    save(&dest.join("after.json"), &after)
}

/// Refuse the entire operation before mutation if any selected path changed after CLI exit.
/// Index restoration uses a private copy and Git's index.lock protocol, preserving other entries.
pub async fn restore(
    env: &ShellEnv,
    dir: &Path,
    id: &str,
    files: &[String],
) -> Result<String, String> {
    if files.is_empty() {
        return Err("Explicit rollback file selection is required".into());
    }
    let dest = directory(env, dir, id).await?;
    let before: Backup = read(&dest.join("before.json"))?;
    let after: ScopeSnapshot = read(&dest.join("after.json"))?;
    let after_modes: BTreeMap<String, u32> = read(&dest.join("after-modes.json"))?;
    let current = scope::snapshot(env, dir).await?;
    let current_modes = modes(&current)?;
    if current.root != before.scope.root
        || current.workdir != before.scope.workdir
        || current.head != before.scope.head
        || after.head != current.head
    {
        return Err("Rollback repository identity/HEAD mismatch".into());
    }
    let prefix = if current.prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", current.prefix)
    };
    let mut paths = std::collections::BTreeSet::new();
    for file in files {
        if file.is_empty()
            || Path::new(file)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || file
                .split('/')
                .any(|c| c.eq_ignore_ascii_case(".git") || c.eq_ignore_ascii_case(".tokage"))
        {
            return Err(format!("Unsafe rollback path: {file}"));
        }
        let path = format!("{prefix}{file}");
        #[cfg(not(unix))]
        if before
            .scope
            .files
            .get(&path)
            .is_some_and(|s| s.disk.starts_with("link:"))
        {
            return Err("Symlink restoration is unsupported on this platform".into());
        }
        if before.scope.files.get(&path) == after.files.get(&path)
            && before.modes.get(&path) == after_modes.get(&path)
        {
            return Err(format!("Not a recorded turn change: {file}"));
        }
        if current.files.get(&path) != after.files.get(&path)
            || current_modes.get(&path) != after_modes.get(&path)
        {
            return Err(format!(
                "File/index changed after execution; preserved: {file}"
            ));
        }
        let target = Path::new(&current.root).join(&path);
        // Absent snapshot entries must also be absent on disk (including newly ignored files).
        if current.files.get(&path).is_none() && fs::symlink_metadata(&target).is_ok() {
            return Err(format!("Unverified file exists: {file}"));
        }
        let mut parent = target.parent().ok_or("Missing file parent")?;
        while !parent.exists() {
            parent = parent.parent().ok_or("Missing ancestor")?;
        }
        if !parent
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(Path::new(&current.root))
        {
            return Err(format!("Rollback parent escapes repository: {file}"));
        }
        paths.insert(path);
    }
    // Validate every original payload before acquiring a lock or touching workspace files.
    for path in &paths {
        if let Some(entry) = before.scope.files.get(path) {
            if entry.disk.starts_with("file:") && !before.modes.contains_key(path) {
                return Err(format!("Missing backup permissions: {path}"));
            }
            if entry.disk.starts_with("file:")
                && !entry.disk.ends_with(&format!(
                    ":{:x}",
                    Sha256::digest(bytes(&dest.join(key(path)))?)
                ))
            {
                return Err(format!("Corrupt rollback payload: {path}"));
            }
        }
    }
    let index = index_path(&dest);
    let lock = index.with_file_name("index.lock");
    let mut guard = IndexLock {
        path: lock.clone(),
        armed: false,
    };
    let mut lock_file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock)
        .map_err(|e| format!("Cannot lock index; nothing restored: {e}"))?;
    guard.armed = true;
    let mut temp_index =
        tempfile::NamedTempFile::new_in(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    temp_index
        .write_all(&bytes(&index)?)
        .map_err(|e| e.to_string())?;
    let temp_path = temp_index.path().to_str().ok_or("Non-UTF8 index path")?;
    for path in &paths {
        let original = before
            .scope
            .files
            .get(path)
            .map(|s| s.index.as_str())
            .unwrap_or("");
        if original.is_empty() {
            run(
                env,
                Path::new(&current.root),
                &["update-index", "--force-remove", "--", path],
                &[("GIT_INDEX_FILE", temp_path)],
                20,
            )
            .await?;
        } else {
            let fields: Vec<_> = original.split(' ').collect();
            if fields.len() != 3 || fields[2] != "0" {
                return Err("Unsupported original index entry".into());
            }
            // Rehydrate original staged bytes even if an intervening GC removed the object.
            let blob = dest.join(format!("object-{}", fields[1]));
            let restored_oid = run(
                env,
                Path::new(&current.root),
                &[
                    "hash-object",
                    "-w",
                    "--no-filters",
                    "--",
                    blob.to_str().ok_or("Non-UTF8 blob backup path")?,
                ],
                &[],
                20,
            )
            .await?;
            if restored_oid.trim() != fields[1] {
                return Err(format!("Corrupt staged backup: {path}"));
            }
            run(
                env,
                Path::new(&current.root),
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    fields[0],
                    fields[1],
                    path,
                ],
                &[("GIT_INDEX_FILE", temp_path)],
                20,
            )
            .await?;
        }
    }
    // Capture current bytes before deletion/overwriting. This also permits manual recovery after partial failure.
    let recovery = tempfile::Builder::new()
        .prefix("recovery-")
        .tempdir_in(&dest)
        .map_err(|e| e.to_string())?
        .keep();
    for path in &paths {
        if current
            .files
            .get(path)
            .is_some_and(|s| s.disk.starts_with("file:"))
        {
            let mut f = fs::File::create(recovery.join(key(path))).map_err(|e| e.to_string())?;
            f.write_all(&bytes(&Path::new(&current.root).join(path))?)
                .and_then(|_| f.sync_all())
                .map_err(|e| e.to_string())?;
        }
    }
    let mut recovery_index = fs::File::create(recovery.join("index")).map_err(|e| e.to_string())?;
    recovery_index
        .write_all(&bytes(&index)?)
        .and_then(|_| recovery_index.sync_all())
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        backup_objects(env, Path::new(&current.root), &recovery, &current),
    )
    .await
    .map_err(|_| "Recovery object backup timed out")??;
    save(&recovery.join("modes.json"), &current_modes)?;
    save(&recovery.join("state.json"), &current)?;
    sync_dir(&dest)?;
    if scope::snapshot(env, dir).await? != current || modes(&current)? != current_modes {
        return Err("Checkout changed during rollback preparation; nothing restored".into());
    }
    let mut restored = Vec::new();
    let result: Result<(), String> = async {
        for path in &paths {
            let latest = scope::snapshot(env, dir).await?;
            if latest.head != current.head
                || latest.files.get(path) != current.files.get(path)
                || modes(&latest)?.get(path) != current_modes.get(path)
            {
                return Err(format!(
                    "File/index changed during rollback; preserved: {path}"
                ));
            }
            let target = Path::new(&current.root).join(path);
            let original = before.scope.files.get(path);
            match original.map(|s| s.disk.as_str()) {
                None | Some("missing") => {
                    if fs::symlink_metadata(&target).is_ok() {
                        fs::remove_file(&target).map_err(|e| e.to_string())?;
                    }
                }
                Some(disk) if disk.starts_with("file:") => {
                    fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
                    let mut tmp = tempfile::NamedTempFile::new_in(target.parent().unwrap())
                        .map_err(|e| e.to_string())?;
                    tmp.write_all(&bytes(&dest.join(key(path)))?)
                        .map_err(|e| e.to_string())?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        tmp.as_file()
                            .set_permissions(fs::Permissions::from_mode(
                                *before
                                    .modes
                                    .get(path)
                                    .ok_or("Missing original permissions")?,
                            ))
                            .map_err(|e| e.to_string())?;
                    }
                    #[cfg(not(unix))]
                    {
                        let mut permissions = tmp
                            .as_file()
                            .metadata()
                            .map_err(|e| e.to_string())?
                            .permissions();
                        permissions.set_readonly(
                            *before
                                .modes
                                .get(path)
                                .ok_or("Missing original permissions")?
                                != 0,
                        );
                        tmp.as_file()
                            .set_permissions(permissions)
                            .map_err(|e| e.to_string())?;
                    }
                    tmp.as_file().sync_all().map_err(|e| e.to_string())?;
                    tmp.persist(&target).map_err(|e| e.to_string())?;
                }
                Some(disk) if disk.starts_with("link:") => {
                    fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
                    if fs::symlink_metadata(&target).is_ok() {
                        fs::remove_file(&target).map_err(|e| e.to_string())?;
                    }
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&disk[5..], &target).map_err(|e| e.to_string())?;
                    #[cfg(not(unix))]
                    return Err("Symlink restoration is unsupported on this platform".into());
                }
                _ => return Err("Unsupported rollback content".into()),
            }
            restored.push(path.clone());
            sync_dir(target.parent().unwrap())?;
        }
        lock_file
            .write_all(&bytes(temp_index.path())?)
            .and_then(|_| lock_file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&lock, &index).map_err(|e| e.to_string())?;
        guard.armed = false;
        sync_dir(index.parent().unwrap())?;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        return Err(format!(
            "Rollback incomplete: {e}; restored paths: {restored:?}; recovery: {}",
            recovery.display()
        ));
    }
    let verified = scope::snapshot(env, dir).await.map_err(|e| {
        format!(
            "Rollback verification failed: {e}; recovery: {}",
            recovery.display()
        )
    })?;
    for path in &paths {
        let original = before.scope.files.get(path);
        let actual = verified.files.get(path);
        // Missing tracked originals may disappear from the final map after index restoration.
        if original != actual || before.modes.get(path) != modes(&verified)?.get(path) {
            return Err(format!(
                "Rollback verification mismatch: {path}; recovery: {}",
                recovery.display()
            ));
        }
    }
    Ok(format!(
        "Restored {} file(s) to turn-start content and index; recovery: {}",
        restored.len(),
        recovery.display()
    ))
}
struct IndexLock {
    path: PathBuf,
    armed: bool,
}
impl Drop for IndexLock {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub async fn ensure_capture(env: &ShellEnv, dir: &Path, id: &str) -> Result<(), String> {
    let dest = directory(env, dir, id).await?;
    let now = scope::snapshot(env, dir).await?;
    if dest.exists() {
        let original: Backup = read(&dest.join("before.json"))?;
        if original.scope != now
            || original.modes != modes(&now)?
            || original.index != bytes(&index_path(&dest))?
            || dest.join("after.json").exists()
            || dest.join("after-modes.json").exists()
        {
            return Err("Existing turn backup does not match launch state".into());
        }
        Ok(())
    } else {
        capture(env, dir, id, &now).await
    }
}

pub async fn changes(env: &ShellEnv, dir: &Path, id: &str) -> Result<Vec<String>, String> {
    let dest = directory(env, dir, id).await?;
    let before: Backup = read(&dest.join("before.json"))?;
    let after: ScopeSnapshot = read(&dest.join("after.json"))?;
    let after_modes: BTreeMap<String, u32> = read(&dest.join("after-modes.json"))?;
    let root = dir.canonicalize().map_err(|e| e.to_string())?;
    if root.to_str() != Some(before.scope.workdir.as_str())
        || before.scope.root != after.root
        || before.scope.head != after.head
    {
        return Err("Rollback identity mismatch".into());
    }
    let prefix = if after.prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", after.prefix)
    };
    let paths: std::collections::BTreeSet<_> = before
        .scope
        .files
        .keys()
        .chain(after.files.keys())
        .collect();
    paths
        .into_iter()
        .filter(|p| {
            before.scope.files.get(*p) != after.files.get(*p)
                || before.modes.get(*p) != after_modes.get(*p)
        })
        .map(|p| {
            p.strip_prefix(&prefix)
                .map(str::to_owned)
                .ok_or_else(|| format!("Turn modified files outside execution directory: {p}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn git(env: &ShellEnv, root: &Path, args: &[&str]) -> String {
        run(env, root, args, &[], 15).await.unwrap()
    }
    async fn repo() -> (tempfile::TempDir, ShellEnv) {
        let tmp = tempfile::tempdir().unwrap();
        let env = ShellEnv {
            vars: std::env::vars().collect(),
        };
        git(&env, tmp.path(), &["init", "-q"]).await;
        git(&env, tmp.path(), &["config", "user.name", "test"]).await;
        git(
            &env,
            tmp.path(),
            &["config", "user.email", "test@example.com"],
        )
        .await;
        git(&env, tmp.path(), &["config", "commit.gpgsign", "false"]).await;
        fs::write(tmp.path().join("a"), "HEAD\n").unwrap();
        git(&env, tmp.path(), &["add", "a"]).await;
        git(&env, tmp.path(), &["commit", "-qm", "base"]).await;
        (tmp, env)
    }
    async fn start(env: &ShellEnv, root: &Path, id: &str) -> ScopeSnapshot {
        let before = scope::snapshot(env, root).await.unwrap();
        capture(env, root, id, &before).await.unwrap();
        before
    }
    #[tokio::test]
    async fn restores_dirty_staged_untracked_binary_deleted_and_renamed_files() {
        let (tmp, env) = repo().await;
        let root = tmp.path();
        fs::write(root.join("a"), b"staged\0bytes").unwrap();
        fs::write(root.join("deleted"), "staged new").unwrap();
        git(&env, root, &["add", "a", "deleted"]).await;
        fs::write(root.join("a"), b"unstaged\0bytes").unwrap();
        fs::write(root.join("user-new"), b"user\0untracked").unwrap();
        let before = start(&env, root, "../unsafe-id").await;
        fs::write(root.join("a"), "agent").unwrap();
        fs::write(root.join("user-new"), "agent replacement").unwrap();
        fs::remove_file(root.join("deleted")).unwrap();
        fs::write(root.join("agent-new"), b"agent\0binary").unwrap();
        git(&env, root, &["add", "-A"]).await;
        seal(&env, root, "../unsafe-id").await.unwrap();
        let files = changes(&env, root, "../unsafe-id").await.unwrap();
        assert_eq!(files.len(), 4);
        let oid = before.files["a"].index.split(' ').nth(1).unwrap();
        let object = root.join(".git/objects").join(&oid[..2]).join(&oid[2..]);
        fs::remove_file(object).unwrap();
        let summary = restore(&env, root, "../unsafe-id", &files).await.unwrap();
        assert_eq!(scope::snapshot(&env, root).await.unwrap(), before);
        assert_eq!(fs::read(root.join("a")).unwrap(), b"unstaged\0bytes");
        assert_eq!(fs::read(root.join("user-new")).unwrap(), b"user\0untracked");
        assert!(!root.join("agent-new").exists());
        let recovery = summary.split("recovery: ").nth(1).unwrap();
        assert_eq!(
            fs::read(Path::new(recovery).join(key("agent-new"))).unwrap(),
            b"agent\0binary"
        );
        assert!(Path::new(recovery).join("index").exists());
        // Rename is two explicit path changes; unrelated index additions survive restoration.
        start(&env, root, "rename").await;
        fs::rename(root.join("a"), root.join("renamed")).unwrap();
        git(&env, root, &["add", "-A"]).await;
        seal(&env, root, "rename").await.unwrap();
        fs::write(root.join("unrelated"), "user later").unwrap();
        git(&env, root, &["add", "unrelated"]).await;
        restore(&env, root, "rename", &["a".into(), "renamed".into()])
            .await
            .unwrap();
        assert_eq!(fs::read(root.join("a")).unwrap(), b"unstaged\0bytes");
        assert!(git(&env, root, &["diff", "--cached", "--name-only"])
            .await
            .contains("unrelated"));
    }
    #[tokio::test]
    async fn rejects_later_disk_or_index_edits_for_the_entire_selection() {
        let (tmp, env) = repo().await;
        let root = tmp.path();
        start(&env, root, "turn").await;
        fs::write(root.join("a"), "agent").unwrap();
        fs::write(root.join("new"), "agent new").unwrap();
        seal(&env, root, "turn").await.unwrap();
        for stage in [false, true] {
            if stage {
                fs::write(root.join("a"), "agent").unwrap();
                git(&env, root, &["add", "a"]).await;
            } else {
                fs::write(root.join("a"), "human").unwrap();
            }
            assert!(restore(&env, root, "turn", &["new".into(), "a".into()])
                .await
                .unwrap_err()
                .contains("changed after execution"));
            assert!(root.join("new").exists());
        }
        assert!(seal(&env, root, "turn").await.is_err());
    }
    #[tokio::test]
    async fn missing_unsealed_corrupt_backup_and_index_lock_never_destroy_work() {
        let (tmp, env) = repo().await;
        let root = tmp.path();
        assert!(restore(&env, root, "missing", &["a".into()]).await.is_err());
        let before = start(&env, root, "turn").await;
        assert!(capture(&env, root, "turn", &before).await.is_err());
        fs::write(root.join("a"), "agent").unwrap();
        assert!(restore(&env, root, "turn", &["a".into()]).await.is_err());
        seal(&env, root, "turn").await.unwrap();
        let dest = directory(&env, root, "turn").await.unwrap();
        fs::write(
            index_path(&dest).with_file_name("index.lock"),
            "other operation",
        )
        .unwrap();
        assert!(restore(&env, root, "turn", &["a".into()])
            .await
            .unwrap_err()
            .contains("lock"));
        fs::remove_file(index_path(&dest).with_file_name("index.lock")).unwrap();
        fs::write(dest.join(key("a")), "corrupt").unwrap();
        assert!(restore(&env, root, "turn", &["a".into()])
            .await
            .unwrap_err()
            .contains("Corrupt"));
        assert_eq!(fs::read_to_string(root.join("a")).unwrap(), "agent");
    }
    #[tokio::test]
    async fn refuses_unsafe_paths_changed_head_and_wrong_repository() {
        let (tmp, env) = repo().await;
        let root = tmp.path();
        start(&env, root, "turn").await;
        fs::write(root.join("a"), "agent").unwrap();
        seal(&env, root, "turn").await.unwrap();
        for path in [
            "../a",
            "/a",
            "a/../a",
            ".git/config",
            ".tokage/context.md",
            "",
        ] {
            assert!(restore(&env, root, "turn", &[path.into()]).await.is_err());
        }
        let (other, _) = repo().await;
        assert!(restore(&env, other.path(), "turn", &["a".into()])
            .await
            .is_err());
        git(&env, root, &["add", "a"]).await;
        git(&env, root, &["commit", "-qm", "external"]).await;
        assert!(restore(&env, root, "turn", &["a".into()])
            .await
            .unwrap_err()
            .contains("HEAD"));
        assert_eq!(fs::read_to_string(root.join("a")).unwrap(), "agent");
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn restores_symlink_and_permissions_without_following_links() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (tmp, env) = repo().await;
        let root = tmp.path();
        fs::write(root.join("b"), "target").unwrap();
        symlink("a", root.join("link")).unwrap();
        fs::set_permissions(root.join("a"), fs::Permissions::from_mode(0o755)).unwrap();
        let before = start(&env, root, "link").await;
        fs::remove_file(root.join("link")).unwrap();
        symlink("b", root.join("link")).unwrap();
        fs::set_permissions(root.join("a"), fs::Permissions::from_mode(0o644)).unwrap();
        seal(&env, root, "link").await.unwrap();
        restore(&env, root, "link", &["a".into(), "link".into()])
            .await
            .unwrap();
        assert_eq!(scope::snapshot(&env, root).await.unwrap(), before);
        assert_eq!(fs::read_link(root.join("link")).unwrap(), Path::new("a"));
        // An escaping parent introduced after the seal is refused by the snapshot reader.
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub/file"), "original").unwrap();
        start(&env, root, "escape").await;
        fs::write(root.join("sub/file"), "agent").unwrap();
        seal(&env, root, "escape").await.unwrap();
        fs::remove_dir_all(root.join("sub")).unwrap();
        symlink(tmp.path().parent().unwrap(), root.join("sub")).unwrap();
        assert!(restore(&env, root, "escape", &["sub/file".into()])
            .await
            .is_err());
    }
    #[tokio::test]
    async fn private_worktree_and_subdirectory_backups_do_not_pollute_git_scope() {
        let (tmp, env) = repo().await;
        let root = tmp.path();
        let outer = tempfile::tempdir().unwrap();
        let wt = outer.path().join("wt");
        git(
            &env,
            root,
            &["worktree", "add", "-qb", "task", wt.to_str().unwrap()],
        )
        .await;
        fs::create_dir(wt.join("sub")).unwrap();
        fs::write(wt.join("sub/file"), "user").unwrap();
        let before = start(&env, &wt.join("sub"), "turn").await;
        assert_eq!(
            scope::snapshot(&env, &wt.join("sub")).await.unwrap(),
            before
        );
        fs::write(wt.join("sub/file"), "agent").unwrap();
        seal(&env, &wt.join("sub"), "turn").await.unwrap();
        assert_eq!(
            changes(&env, &wt.join("sub"), "turn").await.unwrap(),
            ["file"]
        );
        restore(&env, &wt.join("sub"), "turn", &["file".into()])
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(wt.join("sub/file")).unwrap(), "user");
        assert!(changes(&env, &wt, "turn").await.is_err());
    }
    #[tokio::test]
    async fn rejects_intent_to_add_without_claiming_staging_is_preserved() {
        let (tmp, env) = repo().await;
        fs::write(tmp.path().join("intent"), "user content").unwrap();
        git(&env, tmp.path(), &["add", "-N", "intent"]).await;
        let before = scope::snapshot(&env, tmp.path()).await.unwrap();
        assert!(capture(&env, tmp.path(), "intent", &before)
            .await
            .unwrap_err()
            .contains("Intent-to-add"));
        assert_eq!(scope::snapshot(&env, tmp.path()).await.unwrap(), before);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn permission_conflict_after_seal_is_detected_and_partial_io_failure_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let (tmp, env) = repo().await;
        let root = tmp.path();
        start(&env, root, "permission").await;
        fs::write(root.join("a"), "agent").unwrap();
        seal(&env, root, "permission").await.unwrap();
        fs::set_permissions(root.join("a"), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(restore(&env, root, "permission", &["a".into()])
            .await
            .unwrap_err()
            .contains("changed after execution"));
        fs::set_permissions(root.join("a"), fs::Permissions::from_mode(0o644)).unwrap();
        start(&env, root, "partial").await;
        fs::write(root.join("a"), "second agent").unwrap();
        fs::create_dir(root.join("z")).unwrap();
        fs::write(root.join("z/new"), "recover me").unwrap();
        seal(&env, root, "partial").await.unwrap();
        // Root can bypass directory permissions, so this OS failure fixture applies to normal users.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        fs::set_permissions(root.join("z"), fs::Permissions::from_mode(0o555)).unwrap();
        let error = restore(&env, root, "partial", &["a".into(), "z/new".into()])
            .await
            .unwrap_err();
        fs::set_permissions(root.join("z"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(error.contains("Rollback incomplete"), "{error}");
        assert!(error.contains("restored paths: [\"a\"]"), "{error}");
        let recovery = error.split("recovery: ").nth(1).unwrap();
        assert_eq!(
            fs::read(Path::new(recovery).join(key("z/new"))).unwrap(),
            b"recover me"
        );
        assert_eq!(fs::read_to_string(root.join("a")).unwrap(), "agent");
        assert_eq!(
            fs::read_to_string(root.join("z/new")).unwrap(),
            "recover me"
        );
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use crate::{
        chat::{self, testing::request},
        providers::ProviderInfo,
    };
    use std::os::unix::fs::PermissionsExt;
    use tokage_core::{Bus, DomainEvent, EventKind, ProviderId, ProviderStatus};

    // Real child processes behind the real adapter; no model services or paid CLI are used.
    #[tokio::test]
    async fn success_failure_and_cancel_seal_before_done_or_ended() {
        for ending in ["success", "failure", "cancel"] {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            let env = ShellEnv {
                vars: std::env::vars().collect(),
            };
            for args in [
                vec!["init", "-q"],
                vec!["config", "user.name", "test"],
                vec!["config", "user.email", "t@example.com"],
                vec!["config", "commit.gpgsign", "false"],
            ] {
                run(&env, root, &args, &[], 15).await.unwrap();
            }
            fs::write(root.join("a"), "user dirty").unwrap();
            run(&env, root, &["add", "a"], &[], 15).await.unwrap();
            run(&env, root, &["commit", "-qm", "base"], &[], 15)
                .await
                .unwrap();
            fs::write(root.join("a"), "user before").unwrap();
            let exe = root
                .join("../")
                .join(format!("fake-{}-{}", std::process::id(), ending));
            let tail = match ending {
                "failure" => "exit 7",
                "cancel" => "exec /bin/sleep 30",
                _ => "exit 0",
            };
            fs::write(&exe, format!("#!/bin/sh\nprintf agent > a\nprintf '%s\\n' '{{\"type\":\"turn.completed\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}}}}'\n{tail}\n")).unwrap();
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
            let provider = ProviderInfo {
                usage_query: crate::usage_query::contract::capability(ProviderId::CodexCli),
                id: ProviderId::CodexCli,
                name: "Test",
                executable: "test",
                featured: true,
                status: ProviderStatus::Ready,
                version: None,
                path: Some(exe.to_str().unwrap().into()),
                auth_method: None,
                models: vec![],
                model_groups: vec![],
                models_source: None,
                account: None,
                guidance: vec![],
                install_methods: vec![],
                trust_args: vec![],
                chat: true,
                adapter: None,
                direct_limit: None,
            };
            let mut req = request(ProviderId::CodexCli);
            req.cwd = root.to_str().unwrap().into();
            req.turn_id = ending.into();
            req.task_code = Some("T1".into());
            req.trusted = vec![ProviderId::CodexCli];
            let bus = Bus::new();
            let mut events = bus.subscribe();
            let (tx, rx) = tokio::sync::watch::channel(false);
            let (cancelled, _) = tokio::join!(
                chat::run_turn(&bus, req, vec![provider], env.clone(), None, Some(rx)),
                async {
                    if ending == "cancel" {
                        for _ in 0..1000 {
                            if fs::read_to_string(root.join("a")).unwrap() == "agent" {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                        assert_eq!(
                            fs::read_to_string(root.join("a")).unwrap(),
                            "agent",
                            "CLI did not start before cancellation"
                        );
                        tx.send(true).unwrap();
                    }
                }
            );
            assert_eq!(cancelled, ending == "cancel");
            let mut done = 0;
            while let Ok(event) = events.try_recv() {
                if matches!(
                    event,
                    DomainEvent::ChatOutput {
                        event: EventKind::Done { .. },
                        ..
                    }
                ) {
                    done += 1;
                    assert!(directory(&env, root, ending)
                        .await
                        .unwrap()
                        .join("after.json")
                        .exists());
                }
            }
            assert_eq!(done, if ending == "success" { 1 } else { 0 });
            assert_eq!(
                changes(&env, root, ending).await.unwrap(),
                ["a"],
                "{ending}"
            );
            restore(&env, root, ending, &["a".into()]).await.unwrap();
            assert_eq!(fs::read_to_string(root.join("a")).unwrap(), "user before");
            fs::remove_file(exe).unwrap();
        }
    }
}
