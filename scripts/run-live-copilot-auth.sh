#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Only auth.getStatus + models.list: no session creation, chat or paid model request.
cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native --locked --lib chat::copilot_live::authentication_without_chat -- --ignored --nocapture
