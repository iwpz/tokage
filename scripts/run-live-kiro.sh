#!/usr/bin/env bash
# Explicit fixed-model acceptance; spends Kiro credits. No provider/model fallback.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
for engine in v2 v3; do
  TOKAGE_KIRO_ENGINE="$engine" cargo test --manifest-path src-tauri/Cargo.toml \
    -p tokage-native kiro_live::haiku_roles --lib -- --ignored --nocapture
done
for engine in v2 v3; do
  TOKAGE_KIRO_TEST_ENGINE="$engine" bash scripts/run-live-desktop.sh KIRO_CLI
done
