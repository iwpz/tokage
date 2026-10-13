#!/usr/bin/env bash
# Uses the locally configured default for explicit verification and DeepSeek Flash for turns.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src-tauri"
cargo test -p tokage-native --lib chat::reasonix_live::deepseek_turn_resume_copy --locked -- --ignored --nocapture
cargo test -p tokage-native --lib chat::reasonix_live::status_balance_without_model_turn --locked -- --ignored --nocapture
