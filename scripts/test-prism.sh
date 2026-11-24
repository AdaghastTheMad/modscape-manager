#!/usr/bin/env bash
# Isolated Prism Launcher for testing exports. Everything lives in
# test-env/prism/ inside this project (git-ignored); your own Prism data
# (~/Library/Application Support/PrismLauncher, ~/.local/share/PrismLauncher,
# the Flatpak copy) is never read, written or launched.
#
#   scripts/test-prism.sh create          make the isolated data folder
#   scripts/test-prism.sh start           open the test Prism (sign in here once to allow launching)
#   scripts/test-prism.sh import FILE     open the test Prism's import dialog for an .mrpack / CurseForge zip
#   scripts/test-prism.sh launch NAME     launch an instance in the test Prism
#   scripts/test-prism.sh status          list test instances and how many mods each has
#   scripts/test-prism.sh logs NAME       show the end of an instance's latest log / crash report
#   scripts/test-prism.sh --clean         delete test-env/ entirely (asks first)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENV_DIR="$ROOT/test-env"
DATA="$ENV_DIR/prism"

# Refuse to ever point at a real Prism data folder.
for real in "$HOME/Library/Application Support/PrismLauncher" "$HOME/.local/share/PrismLauncher" "$HOME/.var/app/org.prismlauncher.PrismLauncher"; do
  case "$DATA" in "$real"|"$real"/*) echo "Refusing: $DATA is your own Prism data."; exit 1 ;; esac
done

find_prism() {
  if [ "$(uname -s)" = Darwin ]; then
    for app in "/Applications/Prism Launcher.app" "$HOME/Applications/Prism Launcher.app"; do
      [ -x "$app/Contents/MacOS/prismlauncher" ] && { echo "$app/Contents/MacOS/prismlauncher"; return; }
    done
  else
    command -v prismlauncher >/dev/null && { command -v prismlauncher; return; }
    for p in "$HOME/Applications/PrismLauncher.AppImage" "$HOME/.local/bin/prismlauncher"; do
      [ -x "$p" ] && { echo "$p"; return; }
    done
  fi
  echo "Prism Launcher wasn't found. Install it from https://prismlauncher.org (not done automatically)." >&2
  exit 1
}

run_prism() { # run_prism ARGS… — always with the isolated data dir, detached
  local bin; bin="$(find_prism)"
  mkdir -p "$DATA"
  nohup "$bin" --dir "$DATA" "$@" >"$ENV_DIR/prism-launcher.log" 2>&1 &
  echo "Started the test Prism (data: test-env/prism). Output: test-env/prism-launcher.log"
}

instance_dir() { # by folder name or display name
  local want="$1"
  for d in "$DATA/instances"/*/; do
    [ -d "$d" ] || continue
    local id; id="$(basename "$d")"
    local name; name="$(grep -m1 '^name=' "$d/instance.cfg" 2>/dev/null | cut -d= -f2-)"
    if [ "$id" = "$want" ] || [ "$name" = "$want" ]; then echo "$d"; return; fi
  done
  echo "No test instance called '$want'. Try: scripts/test-prism.sh status" >&2
  exit 1
}

case "${1:-}" in
  create)
    mkdir -p "$DATA"
    echo "Isolated Prism data folder ready: test-env/prism"
    ;;
  start)
    run_prism --show-window
    echo "First time: add your Microsoft account in this test Prism yourself (Accounts → Add Microsoft). It's stored only in test-env/prism."
    ;;
  import)
    [ -f "${2:-}" ] || { echo "usage: $0 import FILE"; exit 2; }
    run_prism --import "$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
    echo "Prism shows its import dialog; confirm it there. Downloads land in test-env/prism only."
    ;;
  launch)
    [ -n "${2:-}" ] || { echo "usage: $0 launch NAME"; exit 2; }
    d="$(instance_dir "$2")"
    run_prism --launch "$(basename "$d")"
    ;;
  status)
    [ -d "$DATA/instances" ] || { echo "No test instances yet."; exit 0; }
    for d in "$DATA/instances"/*/; do
      [ -f "$d/instance.cfg" ] || continue
      name="$(grep -m1 '^name=' "$d/instance.cfg" | cut -d= -f2-)"
      mods="$d/minecraft/mods"; [ -d "$mods" ] || mods="$d/.minecraft/mods"
      n=$(find "$mods" -maxdepth 1 -name '*.jar' 2>/dev/null | wc -l | tr -d ' ')
      printf '  %-40s %s jars  (%s)\n' "$name" "$n" "$(basename "$d")"
    done
    ;;
  logs)
    [ -n "${2:-}" ] || { echo "usage: $0 logs NAME"; exit 2; }
    d="$(instance_dir "$2")"
    game="$d/minecraft"; [ -d "$game" ] || game="$d/.minecraft"
    crash="$(ls -t "$game/crash-reports"/*.txt 2>/dev/null | head -1 || true)"
    [ -n "$crash" ] && { echo "== newest crash report: $crash"; head -60 "$crash"; }
    [ -f "$game/logs/latest.log" ] && { echo "== latest.log (last 80 lines)"; tail -80 "$game/logs/latest.log"; }
    ;;
  --clean|clean)
    [ -d "$ENV_DIR" ] || { echo "Nothing to clean."; exit 0; }
    du -sh "$ENV_DIR" 2>/dev/null | awk '{print "test-env/ uses " $1}'
    read -r -p "Delete test-env/ (the test Prism, its account sign-in, downloads)? [y/N] " a </dev/tty
    [[ "$a" =~ ^[Yy] ]] && rm -rf "$ENV_DIR" && echo "Deleted test-env/."
    ;;
  *)
    sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
