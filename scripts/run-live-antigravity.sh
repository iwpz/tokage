#!/usr/bin/env bash
# Real Google account acceptance, using only Gemini Flash Low. No fallback models.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src-tauri"
cargo test -p tokage-native --lib chat::antigravity_live::authenticated_quota --locked -- --ignored --nocapture
cargo test -p tokage-native --lib chat::antigravity_live::google_plan_execute_resume_review --locked -- --ignored --nocapture
