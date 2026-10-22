//! CLI data location only. Legacy run JSON is immutable history, never runtime authority.
use std::path::PathBuf;
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
}
