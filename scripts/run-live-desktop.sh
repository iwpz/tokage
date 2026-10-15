#!/usr/bin/env bash
# Real rendered desktop UI, fixed models, isolated app data/repositories.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# Restore the ordinary frontend even if a live provider fails.
trap 'result=$?; npm run build || result=$?; exit "$result"' EXIT
if [[ $# -eq 0 ]]; then
  set -- CURSOR_CLI COPILOT_CLI ANTIGRAVITY_CLI
fi
VITE_TOKAGE_LIVE_ACCEPTANCE=1 npm run build
cargo build --manifest-path src-tauri/Cargo.toml --bin tokage --features custom-protocol,live-acceptance --locked
status=0
for provider in "$@"; do
  runner=(python3 scripts/run-live-desktop.py "$provider")
  if [[ "$provider" == "KIRO_CLI" ]]; then runner+=(--kiro-engine "${TOKAGE_KIRO_TEST_ENGINE:-v2}"); fi
  if [[ "$provider" == "OPENCODE" && -n "${TOKAGE_OPENCODE_TEST_BIN:-}" ]]; then
    runner+=(--cli-path "$TOKAGE_OPENCODE_TEST_BIN" --version-prefix "${TOKAGE_OPENCODE_TEST_VERSION:-1.}" --opencode-v1-auth)
  fi
  if "${runner[@]}"; then
    echo "PASS $provider"
  else
    echo "FAIL $provider"
    status=1
  fi
done
exit "$status"
