#!/usr/bin/env bash
# Official Claude subscription only, fixed Haiku; missing auth or real failures fail.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
for test in claude_haiku_role_chain claude_subscription_usage; do
  cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native --lib "chat::baseline_live::$test" --locked -- --ignored --nocapture
done
scripts/run-live-desktop.sh CLAUDE_CODE
