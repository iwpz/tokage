#!/usr/bin/env bash
# Paid real-model acceptance: natural request → rendered review → parallel DAG → independent review → merge.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
trap 'result=$?; npm run build || result=$?; exit "$result"' EXIT
VITE_TOKAGE_LIVE_ACCEPTANCE=1 npm run build
cargo build --manifest-path src-tauri/Cargo.toml --bin tokage --features custom-protocol,live-acceptance --locked
if [[ $# -eq 0 ]]; then set -- CLAUDE_CODE OPENCODE; fi
status=0
for provider in "$@"; do
  case "$provider" in
    CLAUDE_CODE) version=2.1.296 ;;
    OPENCODE) version=2.0.29 ;;
    *) echo "Unsupported m2b target: $provider" >&2; exit 2 ;;
  esac
  runner=(python3 scripts/run-live-desktop.py "$provider" --scenario natural --version-prefix "$version" --timeout 1500)
  if [[ "$provider" == "CLAUDE_CODE" ]]; then runner+=(--background-probe-seconds "${TOKAGE_BACKGROUND_PROBE_SECONDS:-310}"); fi
  if [[ "$provider" == "CLAUDE_CODE" && -n "${TOKAGE_CLAUDE_TEST_BIN:-}" ]]; then runner+=(--cli-path "$TOKAGE_CLAUDE_TEST_BIN"); fi
  if [[ "$provider" == "OPENCODE" && -n "${TOKAGE_OPENCODE_TEST_BIN:-}" ]]; then runner+=(--cli-path "$TOKAGE_OPENCODE_TEST_BIN"); fi
  if "${runner[@]}"; then
    echo "PASS natural $provider"
  else
    echo "FAIL natural $provider"
    status=1
  fi
done
exit "$status"
