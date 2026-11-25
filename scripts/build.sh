#!/usr/bin/env bash
# Modscape Manager — build the installers.
#
#   macOS:  .dmg (plus the .app inside it)
#   Linux:  .deb and .AppImage (Linux Mint / Ubuntu; build on Linux)
#
# Checks the toolchain first (scripts/setup.sh --check) and never installs
# anything itself. Runs the tests, builds a release, and copies the
# installers to installers/ (git-ignored).
#
#   scripts/build.sh               check, test, build
#   scripts/build.sh --skip-tests  build only (still checks the toolchain)
#   scripts/build.sh --yes         don't ask before the AppImage tool download
set -euo pipefail

SKIP_TESTS=0
YES=0
for a in "$@"; do
  case "$a" in
    --skip-tests) SKIP_TESTS=1 ;;
    --yes|-y) YES=1 ;;
    -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
    *) echo "usage: $0 [--skip-tests] [--yes]"; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OS="$(uname -s)"
step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
die()  { printf '\033[31m✘ %s\033[0m\n' "$*"; exit 1; }

case "$OS" in
  Darwin) BUNDLES="dmg" ;;
  Linux)  BUNDLES="deb,appimage" ;;
  *) die "Installers are built on macOS (.dmg) or Linux (.deb, .AppImage) only." ;;
esac

# rustup's cargo, as in dev.sh
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

step "Checking the toolchain"
if ! scripts/setup.sh --check; then
  die "Something is missing (see above). Run scripts/setup.sh to install it, then build again."
fi

if [ ! -d node_modules ]; then
  step "Installing JavaScript packages (pnpm, from the lockfile)"
  pnpm install --frozen-lockfile
fi

if [ "$SKIP_TESTS" = 0 ]; then
  step "Tests"
  pnpm typecheck
  (cd src-tauri && cargo test --quiet)
fi

if [ "$OS" = Linux ] && [[ "$BUNDLES" == *appimage* ]] && [ "$YES" = 0 ]; then
  # Tauri's AppImage step fetches linuxdeploy into ~/.cache/tauri the first time.
  if [ ! -d "$HOME/.cache/tauri" ] || [ -z "$(ls -A "$HOME/.cache/tauri" 2>/dev/null)" ]; then
    printf '\nThe AppImage needs Tauri to download its linuxdeploy tools (from GitHub, into ~/.cache/tauri) once.\n'
    read -r -p "Download them now? (No builds the .deb only) [y/N] " a </dev/tty || a=n
    [[ "$a" =~ ^[Yy] ]] || BUNDLES="deb"
  fi
fi

step "Building the release ($BUNDLES)"
# CI=true: the .dmg step skips its Finder window-layout script (which would ask
# for permission to control Finder) and builds the plain image.
if [ "$OS" = Linux ]; then
  # linuxdeploy (the AppImage step) is itself an AppImage: extract-and-run means
  # no FUSE is needed to build, and skipping strip avoids its failure on the
  # newer ELF sections Ubuntu 24.04 / Mint 22 produce.
  export APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=true
fi
CI=true pnpm tauri build --bundles "$BUNDLES"

step "Collecting installers"
VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
OUT="$ROOT/installers"
mkdir -p "$OUT"
BUNDLE_DIR="src-tauri/target/release/bundle"
found=0
for f in "$BUNDLE_DIR"/dmg/*.dmg "$BUNDLE_DIR"/deb/*.deb "$BUNDLE_DIR"/appimage/*.AppImage; do
  [ -f "$f" ] || continue
  cp -f "$f" "$OUT/"
  printf '  %s  (%s)\n' "$OUT/$(basename "$f")" "$(du -h "$f" | cut -f1)"
  found=$((found+1))
done
[ "$found" -gt 0 ] || die "The build finished but no installer was found in $BUNDLE_DIR."

if [ "$OS" = Darwin ]; then
  cat <<EOF

Modscape Manager $VERSION is built. The app isn't signed with an Apple
Developer ID, so the first time you open it macOS will refuse: right-click
(or Control-click) the app in Applications → Open → Open. After that it
opens normally.
EOF
else
  cat <<EOF

Modscape Manager $VERSION is built.
  .deb:      sudo apt install ./installers/<file>.deb   (asks for your password)
  .AppImage: chmod +x installers/<file>.AppImage, then run it
EOF
fi
