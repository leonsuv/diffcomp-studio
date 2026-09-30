#!/usr/bin/env bash
set -euo pipefail
# Compatibility entry point: pure Rust builds no longer need copied OpenCV/PDFium libraries.
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec bash "$PROJECT_ROOT/scripts/bundle-macos.sh"
