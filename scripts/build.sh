#!/bin/sh
# Put the herd-gamepad binary at bin/herd-gamepad (or $HERD_GAMEPAD_OUT). An installed copy
# that already has its binary and ships no sources does nothing. Needs Rust (rustup.rs).
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out=${HERD_GAMEPAD_OUT:-"$root/bin/herd-gamepad"}
if [ -z "${HERD_GAMEPAD_OUT:-}" ] && [ -x "$out" ] && [ ! -f "$root/Cargo.toml" ]; then
  exit 0
fi
command -v cargo >/dev/null 2>&1 || { echo "herd-gamepad: cargo not found; install Rust from https://rustup.rs" >&2; exit 1; }
( cd "$root" && cargo build --locked --release --quiet )
mkdir -p "$(dirname -- "$out")"
# A fresh inode avoids invalidating a running executable and macOS signature caches.
staged=$(mktemp "${out}.XXXXXX")
trap 'rm -f "$staged"' EXIT HUP INT TERM
cp "$root/target/release/herd-gamepad" "$staged"
chmod +x "$staged"
mv -f "$staged" "$out"
