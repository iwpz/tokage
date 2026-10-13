#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Explicitly billed acceptance: only --model auto --auto-tier efficiency, no model fallback.
cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native --locked --lib chat::copilot_live::auto_efficiency_plan_edit_resume_review -- --ignored --nocapture

cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native --locked --lib chat::copilot_live::interactive_usage -- --ignored --nocapture
