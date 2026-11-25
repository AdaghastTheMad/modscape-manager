#!/usr/bin/env bash
# Modpack Planner — development environment setup.
# Checks every dependency first, reports what's missing, and asks before
# installing anything (and separately before any sudo). Safe to re-run.
#
#   scripts/setup.sh          interactive
#   scripts/setup.sh --check  report only, never install
#   scripts/setup.sh --yes    install missing items without per-item prompts
#                             (sudo still prompts for your password itself)
set -euo pipefail

MODE=ask
case "${1:-}" in
  --check) MODE=check ;;
  --yes|-y) MODE=yes ;;
  "") ;;
  *) echo "usage: $0 [--check|--yes]"; exit 2 ;;
esac

ok()   { printf '  \033[32m✔\033[0m %s\n' "$*"; }
miss() { printf '  \033[33m✘\033[0m %s\n' "$*"; MISSING=$((MISSING+1)); }
info() { printf '  · %s\n' "$*"; }
MISSING=0
INSTALLED=0

confirm() { # confirm "question" -> 0 if yes
  [ "$MODE" = check ] && return 1
  [ "$MODE" = yes ] && return 0
  local a; read -r -p "    $1 [y/N] " a </dev/tty || return 1
  [[ "$a" =~ ^[Yy] ]]
}

need_node_major=20   # Vite / Tauri CLI floor; any current LTS is fine
need_rust="1.90.0"     # current Tauri 2 crates need 1.90+

ver_ge() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -1)" = "$2" ]; }

OS="$(uname -s)"
echo "Modpack Planner setup — $OS"

# ---------------------------------------------------------------- platform libs
if [ "$OS" = Darwin ]; then
  echo "System:"
  if xcode-select -p >/dev/null 2>&1; then ok "Xcode Command Line Tools ($(xcode-select -p))"
  else
    miss "Xcode Command Line Tools"
    if confirm "Run 'xcode-select --install' (opens Apple's installer)?"; then
      xcode-select --install || true
      info "Finish the Apple installer, then re-run this script."; exit 0
    fi
  fi
  if command -v brew >/dev/null; then ok "Homebrew $(brew --version | head -1 | awk '{print $2}')"
  else miss "Homebrew (optional here; see https://brew.sh — not installed automatically)"; fi

elif [ "$OS" = Linux ]; then
  echo "System (apt — Linux Mint / Ubuntu):"
  if ! command -v apt-get >/dev/null; then
    echo "  Only apt-based distros are supported."; exit 1
  fi
  # Tauri 2 prerequisites: https://v2.tauri.app/start/prerequisites/#linux
  APT_PKGS=(libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev
            libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config
            libdbus-1-dev gnome-keyring)
  APT_MISSING=()
  for p in "${APT_PKGS[@]}"; do
    if dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q "install ok installed"; then ok "$p"
    else miss "$p"; APT_MISSING+=("$p"); fi
  done
  if [ ${#APT_MISSING[@]} -gt 0 ]; then
    echo "    Needs: sudo apt-get install ${APT_MISSING[*]}"
    if confirm "Install these with sudo apt-get? (sudo will ask for your password)"; then
      sudo apt-get update && sudo apt-get install -y "${APT_MISSING[@]}" && INSTALLED=$((INSTALLED+1))
    fi
  fi
else
  echo "Unsupported OS: $OS (Linux Mint and macOS only)."; exit 1
fi

# ---------------------------------------------------------------- Rust
echo "Rust:"
if command -v rustup >/dev/null; then
  ok "rustup $(rustup --version 2>/dev/null | awk '{print $2}')"
  RV="$(rustc --version | awk '{print $2}')"
  if ver_ge "$RV" "$need_rust"; then ok "rustc $RV"
  else
    miss "rustc $RV is older than $need_rust"
    confirm "Run 'rustup update stable'?" && rustup update stable && INSTALLED=$((INSTALLED+1))
  fi
else
  if command -v rustc >/dev/null; then
    RV="$(rustc --version | awk '{print $2}')"
    info "rustc $RV found without rustup ($(command -v rustc))."
    if ver_ge "$RV" "$need_rust"; then
      info "It's new enough, but rustup is recommended so the toolchain can be updated."
    else
      info "It's older than the $need_rust this project needs; rustup will provide a current one."
    fi
  fi
  miss "rustup"
  if confirm "Install Rust via the official rustup installer (https://sh.rustup.rs, user-local, no sudo)?"; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
    INSTALLED=$((INSTALLED+1))
    info "Add  . \"\$HOME/.cargo/env\"  to your shell profile so new terminals find cargo."
    if command -v brew >/dev/null && brew list rust >/dev/null 2>&1; then
      info "Homebrew's rust is also installed; rustup's comes first only if ~/.cargo/bin is earlier on PATH."
      info "Optionally remove it later with: brew uninstall rust"
    fi
  fi
fi

# ---------------------------------------------------------------- Node + pnpm
echo "Node / pnpm:"
if command -v node >/dev/null; then
  NV="$(node --version | sed 's/^v//')"
  if [ "${NV%%.*}" -ge "$need_node_major" ]; then ok "node $NV"
  else miss "node $NV is older than $need_node_major"; info "Upgrade to the current LTS (https://nodejs.org or your version manager)."; fi
else
  miss "node"
  if [ "$OS" = Darwin ] && command -v brew >/dev/null; then
    confirm "Install Node LTS with 'brew install node@22'?" && brew install node@22 && INSTALLED=$((INSTALLED+1))
  else
    info "Install Node LTS, e.g. via nvm: https://github.com/nvm-sh/nvm (not done automatically)."
  fi
fi

if command -v pnpm >/dev/null; then ok "pnpm $(pnpm --version)"
else
  miss "pnpm"
  if command -v corepack >/dev/null; then
    confirm "Enable pnpm via 'corepack enable pnpm' (no sudo when Node is user-installed)?" \
      && corepack enable pnpm && INSTALLED=$((INSTALLED+1))
  else
    info "Install pnpm: https://pnpm.io/installation (not done automatically)."
  fi
fi

# ---------------------------------------------------------------- project deps
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
echo "Project:"
if [ -d "$ROOT/node_modules" ]; then ok "node_modules present"
elif command -v pnpm >/dev/null; then
  miss "node_modules"
  confirm "Run 'pnpm install' in the project?" && (cd "$ROOT" && pnpm install) && INSTALLED=$((INSTALLED+1))
fi

echo
if [ "$MISSING" -eq 0 ]; then echo "All good — run scripts/dev.sh to start the app."
elif [ "$INSTALLED" -gt 0 ]; then echo "Installed $INSTALLED item(s). Re-run this script to confirm everything is in place."
else echo "$MISSING item(s) missing. Re-run without --check to install them."; fi
# --check reports through the exit code too (scripts/build.sh relies on it).
if [ "$MODE" = check ] && [ "$MISSING" -gt 0 ]; then exit 1; fi
