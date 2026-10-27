#!/usr/bin/env bash
# install.sh — build herd-gamepad, copy it to a stable folder and link it into Herdr.
#   install.sh [--dest DIR] [--dry-run]
# Re-run to update. Settings and state are untouched (Herdr's per-plugin folders).
set -euo pipefail
src="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
dest="${HOME}/.local/share/herdr-plugins/gamepad"
dry=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --dest) dest="$2"; shift 2 ;;
    --dry-run) dry=1; shift ;;
    *) echo "usage: install.sh [--dest DIR] [--dry-run]" >&2; exit 2 ;;
  esac
done
case "$dest" in /*) ;; *) dest="$PWD/$dest" ;; esac
marker=".herd-gamepad-install"
if [ -e "$dest" ] && [ ! -f "$dest/$marker" ]; then
  echo "install.sh: $dest exists and is not a herd-gamepad install; choose --dest" >&2
  exit 1
fi
if [ "$dry" = 1 ]; then
  echo "would: build bin/herd-gamepad, copy herdr-plugin.toml README.md scripts/build.sh to $dest, then: herdr plugin link $dest"
  exit 0
fi
mkdir -p "$(dirname "$dest")"
tmp="$(mktemp -d "$dest.new.XXXXXX")"
cp "$src/herdr-plugin.toml" "$src/README.md" "$tmp/"
mkdir -p "$tmp/scripts" && cp "$src/scripts/build.sh" "$tmp/scripts/build.sh"
HERD_GAMEPAD_OUT="$tmp/bin/herd-gamepad" /bin/sh "$src/scripts/build.sh"
( cd "$src" && git rev-parse --short HEAD 2>/dev/null || echo unknown ) > "$tmp/$marker"
rm -rf "$dest.prev"
[ -e "$dest" ] && mv "$dest" "$dest.prev"
mv "$tmp" "$dest"
rm -rf "$dest.prev"
herdr plugin unlink herd.gamepad >/dev/null 2>&1 || true
herdr plugin link "$dest"
echo "installed to $dest and linked; run the action 'Gamepad: set up voice', then 'Gamepad: start'"
