#!/bin/sh
# Put the herd-gamepad binary at bin/herd-gamepad (or $HERD_GAMEPAD_OUT). An installed copy
# that already has its binary and ships no sources does nothing.
# Order: the GitHub release binary for this platform and the manifest's version, checked
# against its SHA-256; then cargo when the download fails. HERD_GAMEPAD_BUILD=source skips
# the download (install.sh sets it so a development checkout builds local changes).
# HERD_GAMEPAD_RELEASE_URL overrides the release base URL; tests point it at a directory.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out=${HERD_GAMEPAD_OUT:-"$root/bin/herd-gamepad"}
if [ -z "${HERD_GAMEPAD_OUT:-}" ] && [ -x "$out" ] && [ ! -f "$root/Cargo.toml" ]; then
  exit 0
fi
repo=PatrickPriestley/herdr-gamepad

# The SHA-256 of a file, from whichever of shasum and sha256sum exists.
sha256_of() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  elif command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else return 1
  fi
}

# Download and verify the release binary into $1; non-zero with the reason on stderr otherwise.
fetch_release() {
  case "$(uname -s) $(uname -m)" in
    "Darwin arm64")  target=aarch64-apple-darwin ;;
    "Darwin x86_64") target=x86_64-apple-darwin ;;
    *) echo "herd-gamepad: no prebuilt binary for $(uname -s) $(uname -m)" >&2; return 1 ;;
  esac
  version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/herdr-plugin.toml")
  asset="herd-gamepad-$target.tar.gz"
  base=${HERD_GAMEPAD_RELEASE_URL:-"https://github.com/$repo/releases/download/v$version"}
  dl=$(mktemp -d)
  if ! curl -fsSL "$base/$asset" -o "$dl/$asset" || ! curl -fsSL "$base/$asset.sha256" -o "$dl/$asset.sha256"; then
    echo "herd-gamepad: no release download for v$version at $base" >&2
    rm -rf "$dl"; return 1
  fi
  want=$(cut -d' ' -f1 < "$dl/$asset.sha256")
  got=$(sha256_of "$dl/$asset") || got=""
  # Fail closed: an empty expected digest never matches, even an empty computed one.
  if [ -z "$want" ] || [ "$want" != "$got" ]; then
    echo "herd-gamepad: checksum mismatch for $asset (want ${want:-nothing}, got ${got:-nothing}); not installing it" >&2
    rm -rf "$dl"; return 1
  fi
  tar -xzf "$dl/$asset" -C "$dl" herd-gamepad && cp "$dl/herd-gamepad" "$1"
  status=$?
  rm -rf "$dl"
  return $status
}

# Build with cargo into $1; non-zero when cargo is not installed.
build_source() {
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  command -v cargo >/dev/null 2>&1 || return 1
  ( cd "$root" && cargo build --locked --release --quiet )
  cp "$root/target/release/herd-gamepad" "$1"
}

mkdir -p "$(dirname -- "$out")"
# A fresh inode avoids invalidating a running executable and macOS signature caches.
staged=$(mktemp "${out}.XXXXXX")
trap 'rm -f "$staged"' EXIT HUP INT TERM
if [ "${HERD_GAMEPAD_BUILD:-}" = source ]; then
  build_source "$staged" || { echo "herd-gamepad: cargo not found; install Rust from https://rustup.rs" >&2; exit 1; }
elif ! fetch_release "$staged"; then
  build_source "$staged" || {
    echo "herd-gamepad: could not download the release binary, and cargo is not installed; check the network or install Rust from https://rustup.rs and retry" >&2
    exit 1
  }
fi
chmod +x "$staged"
mv -f "$staged" "$out"
