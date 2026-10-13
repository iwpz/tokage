#!/usr/bin/env bash
# Cursor subscription acceptance: only --model auto, no gateway/API-key model requests.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src-tauri"
cargo test -p tokage-native --lib chat::cursor_live::invalid_key_cannot_borrow_subscription_login --locked -- --ignored --nocapture
cargo test -p tokage-native --lib chat::cursor_live::auto_plan_execute_resume_review --locked -- --ignored --nocapture
