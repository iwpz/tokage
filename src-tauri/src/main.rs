// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = tokage_native::chat::run_reasonix_sandbox_helper() {
        std::process::exit(code);
    }
    tokage_lib::run()
}
