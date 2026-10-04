use std::process::Command;

/// Injects build provenance for Settings → About and source links (docs/11 §7.1):
/// `TOKAGE_COMMIT` (full hash, empty outside a git checkout) and `TOKAGE_BUILD_KIND`
/// (`official` only when the release pipeline sets `TOKAGE_OFFICIAL_BUILD=1`).
fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let official = std::env::var("TOKAGE_OFFICIAL_BUILD").is_ok_and(|v| v == "1");
    println!("cargo:rustc-env=TOKAGE_COMMIT={commit}");
    println!(
        "cargo:rustc-env=TOKAGE_BUILD_KIND={}",
        if official { "official" } else { "local" }
    );
    println!("cargo:rerun-if-env-changed=TOKAGE_OFFICIAL_BUILD");
    // HEAD moves on checkout; the ref file it points to moves on commit.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    if let Ok(head) = std::fs::read_to_string("../.git/HEAD") {
        if let Some(r) = head.trim().strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed=../.git/{r}");
        }
    }
    tauri_build::build()
}
