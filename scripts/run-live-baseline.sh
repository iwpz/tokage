#!/usr/bin/env bash
# Fixed Haiku / gpt-6-luna acceptance; a missing or unauthenticated CLI fails.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src-tauri"
status=0
for test in claude_haiku_role_chain codex_luna_role_chain; do
  if cargo test -p tokage-native --lib "chat::baseline_live::$test" --locked -- --ignored --nocapture; then
    echo "PASS $test"
  else
    echo "FAIL $test"
    status=1
  fi
done
exit "$status"
