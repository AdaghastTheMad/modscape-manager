#!/usr/bin/env bash
# Record real CurseForge responses for the resolver tests.
# Reads the CurseForge key saved in the app (macOS may ask whether this tool
# may read it), or CURSEFORGE_API_KEY if set. Only talks to api.curseforge.com.
set -euo pipefail
cd "$(dirname "$0")/../src-tauri"
# Prefer rustup's toolchain even if the shell profile doesn't load it yet.
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
exec cargo run -q --example record_cf_fixtures
