#!/usr/bin/env bash
# Modpack Planner — one-stop Linux build (Linux Mint / Ubuntu).
#
# Run it from the project folder on the Linux machine:
#   scripts/linux-build.sh
#
# What it does, asking before anything that installs or uses sudo:
#   1. Checks the system packages, Rust, Node and pnpm (scripts/setup.sh),
#      offering to install what's missing. Node can be installed user-local
#      with nvm if you don't have it.
#   2. Builds the .deb and .AppImage (scripts/build.sh), with the tests first.
#   3. Offers to install the .deb.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
say()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
ask()  { local a; read -r -p "$1 [y/N] " a </dev/tty || return 1; [[ "$a" =~ ^[Yy] ]]; }
die()  { printf '\033[31m✘ %s\033[0m\n' "$*"; exit 1; }

[ "$(uname -s)" = Linux ] || die "This script is for Linux. On macOS, run scripts/build.sh."
command -v apt-get >/dev/null || die "Only apt-based distros (Linux Mint, Ubuntu) are supported."
if [ -r /etc/os-release ]; then . /etc/os-release; echo "Building on ${PRETTY_NAME:-Linux}"; fi

# Load tools installed in earlier runs.
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"

# ---------------------------------------------------------------- 1. toolchain
say "1/3 · Checking the toolchain"
if ! command -v node >/dev/null; then
  echo "Node.js isn't installed. The app's interface is built with it."
  if ask "Install Node.js LTS for your user only, with nvm (downloads from github.com/nvm-sh and nodejs.org, no sudo)?"; then
    curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | PROFILE=/dev/null bash
    . "$NVM_DIR/nvm.sh"
    nvm install --lts
    echo "Node $(node --version) installed. To use it in new terminals, add to ~/.bashrc:"
    echo '  export NVM_DIR="$HOME/.nvm"; [ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"'
  else
    die "Install Node.js 20 or newer (see docs/LINUX.md), then run this again."
  fi
fi

# setup.sh asks before each install (and before sudo); run it until nothing is missing.
for attempt in 1 2 3; do
  if scripts/setup.sh --check >/dev/null 2>&1; then break; fi
  scripts/setup.sh || true
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if [ "$attempt" = 3 ]; then
    scripts/setup.sh --check || die "Some things are still missing (above). Install them, then run this again."
  fi
done
echo "Toolchain ready."

# ---------------------------------------------------------------- 2. build
say "2/3 · Building"
scripts/build.sh "$@"

# ---------------------------------------------------------------- 3. install
say "3/3 · Install"
DEB="$(ls -t installers/*.deb 2>/dev/null | head -1 || true)"
IMG="$(ls -t installers/*.AppImage 2>/dev/null | head -1 || true)"
if [ -n "$DEB" ] && ask "Install $(basename "$DEB") now? (sudo apt will ask for your password)"; then
  sudo apt install -y "./$DEB"
  echo "Installed. Find Modpack Planner in your applications menu (Utility), or run: modpack-planner"
else
  [ -n "$DEB" ] && echo "Install it later with:  sudo apt install ./$DEB"
fi
if [ -n "$IMG" ]; then
  chmod +x "$IMG"
  echo "Or run the AppImage directly:  ./$IMG   (it needs libfuse2; see docs/LINUX.md)"
fi
