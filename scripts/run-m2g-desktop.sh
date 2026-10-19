#!/usr/bin/env bash
# Paid desktop cold-recovery acceptance; exact selected CLI versions only.
set -euo pipefail
if [[ "${1:-}" != "--allow-paid" ]]; then
  echo "m2g sends real model requests; pass --allow-paid" >&2
  exit 2
fi
shift
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
python3 scripts/check-cli-baselines.py
trap 'result=$?; npm run build || result=$?; exit "$result"' EXIT
VITE_TOKAGE_LIVE_ACCEPTANCE=1 npm run build
cargo build --manifest-path src-tauri/Cargo.toml --bin tokage --features custom-protocol,live-acceptance --locked
if [[ $# -eq 0 ]]; then set -- CLAUDE_CODE OPENCODE; fi
status=0
for provider in "$@"; do
  case "$provider" in
    CLAUDE_CODE) cli="${TOKAGE_CLAUDE_TEST_BIN:-$(command -v claude)}" ;;
    OPENCODE) cli="${TOKAGE_OPENCODE_TEST_BIN:-$(command -v opencode)}" ;;
    *) echo "Unsupported m2g target: $provider" >&2; exit 2 ;;
  esac
  if python3 scripts/run-m2g-crash.py "$provider" --allow-paid --cli-path "$cli"; then
    echo "PASS cold-recovery $provider"
  else
    echo "FAIL cold-recovery $provider"
    status=1
  fi
done
exit "$status"
