#!/usr/bin/env bash
# Run ignored real-CLI E2E tests (skip-if-absent per provider).
# Not part of default CI — requires local official CLIs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src-tauri"

echo "== chat::live smoke (non-ignored; skip-if-absent) =="
cargo test -p tokage-native --lib chat::live::smoke_detect_skip_if_absent --locked -- --nocapture

echo "== chat::live full turns (ignored; set TOKAGE_LIVE_SMOKE=1 for opt-in turn in smoke) =="
cargo test -p tokage-native --lib chat::live --locked -- --ignored --nocapture

echo "== providers::live (ignored) =="
cargo test -p tokage-native --lib providers::live --locked -- --ignored --nocapture

echo "Done. Missing CLIs print 'skip …' and do not fail the suite."
