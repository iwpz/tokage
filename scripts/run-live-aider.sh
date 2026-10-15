#!/usr/bin/env bash
# Fixed configured DeepSeek Flash; real API failures never count as success.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native --lib chat::aider_live::deepseek_chat_edit_resume_review --locked -- --ignored --nocapture
scripts/run-live-desktop.sh AIDER
