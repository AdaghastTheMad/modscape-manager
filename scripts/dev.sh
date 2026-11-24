#!/usr/bin/env bash
# Run Modpack Planner in development mode with hot reload.
set -euo pipefail
cd "$(dirname "$0")/.."
# Prefer rustup's toolchain even if the shell profile doesn't load it yet.
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
[ -d node_modules ] || pnpm install
exec pnpm tauri dev "$@"
