#!/usr/bin/env bash
set -euo pipefail
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
command -v trunk >/dev/null || { echo 'Install trunk with cargo install trunk --locked.' >&2; exit 1; }
rustup target add wasm32-unknown-unknown
cd "$PROJECT_ROOT/crates/dc_app"
env -u NO_COLOR trunk build --release --locked
printf 'Web build ready: %s\n' "$PWD/dist"
