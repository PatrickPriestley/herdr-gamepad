#!/bin/sh
# Put the herd-gamepad binary at bin/herd-gamepad (or $HERD_GAMEPAD_OUT). An installed copy
# that already has its binary and ships no sources does nothing.
# Order: the GitHub release binary for this platform and version, verified by SHA-256
# and its embedded source commit against a clean Git checkout; otherwise build with cargo. HERD_GAMEPAD_BUILD=source skips
# the download (install.sh sets it so a development checkout builds local changes); any other
# value is an error. HERD_GAMEPAD_RELEASE_URL overrides the release base URL; tests point it
# at a directory.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
out=${HERD_GAMEPAD_OUT:-"$root/bin/herd-gamepad"}
if [ -z "${HERD_GAMEPAD_OUT:-}" ] && [ -x "$out" ] && [ ! -f "$root/Cargo.toml" ]; then
  exit 0
fi
case "${HERD_GAMEPAD_BUILD:-}" in
  ""|source) ;;
  *) echo "herd-gamepad: unknown HERD_GAMEPAD_BUILD value \"$HERD_GAMEPAD_BUILD\"; use source or leave it unset" >&2; exit 1 ;;
esac
repo=PatrickPriestley/herdr-gamepad

# The SHA-256 of a file, from whichever tool exists. Capture the tool first so a
# successful cut cannot hide a failing checksum command.
sha256_of() {
  if command -v shasum >/dev/null 2>&1; then digest=$(shasum -a 256 "$1") || return 1
  elif command -v sha256sum >/dev/null 2>&1; then digest=$(sha256sum "$1") || return 1
  else return 1
  fi
  printf '%s\n' "$digest" | cut -d' ' -f1
}

# Git identity is useful only for this checkout, with no local source changes.
# A source archive or a directory nested in another repository must build from source.
checkout_revision() {
  checkout_root=$(git -C "$root" rev-parse --show-toplevel 2>/dev/null) || return 1
  [ "$checkout_root" = "$root" ] || return 1
  checkout_head=$(git -C "$root" rev-parse --verify HEAD 2>/dev/null) || return 1
  changes=$(git -C "$root" status --porcelain --untracked-files=normal 2>/dev/null) || return 1
  if [ -n "$changes" ]; then
    echo "herd-gamepad: checkout is not clean; building its local sources" >&2
    return 1
  fi
  printf '%s\n' "$checkout_head"
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
  # An explicit template: macOS mktemp ignores TMPDIR without one.
  dl=$(mktemp -d "${TMPDIR:-/tmp}/herd-gamepad.XXXXXX")
  # Timeouts turn a stalled connection into a cargo fallback instead of a hung install.
  if ! curl -fsSL --connect-timeout 15 --max-time 300 "$base/$asset" -o "$dl/$asset" ||
     ! curl -fsSL --connect-timeout 15 --max-time 300 "$base/$asset.sha256" -o "$dl/$asset.sha256"; then
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
  if ! tar -xzf "$dl/$asset" -C "$dl" herd-gamepad herd-gamepad.commit; then
    echo "herd-gamepad: release asset $asset did not contain a usable herd-gamepad binary" >&2
    rm -rf "$dl"; return 1
  fi
  # Downloads may take minutes; validate the checkout after downloading and extracting.
  revision=$(checkout_revision) || {
    echo "herd-gamepad: cannot verify a clean Git checkout; building from source" >&2
    rm -rf "$dl"; return 1
  }
  if ! built_revision=$(cat "$dl/herd-gamepad.commit") || [ "$built_revision" != "$revision" ]; then
    echo "herd-gamepad: release asset source commit mismatch; building this checkout" >&2
    rm -rf "$dl"; return 1
  fi
  if ! cp "$dl/herd-gamepad" "$1"; then
    echo "herd-gamepad: could not stage the release binary" >&2
    rm -rf "$dl"; return 1
  fi
  rm -rf "$dl"
}

# Build with cargo into $1; 127 when cargo is not installed, 1 when the build fails. Every
# step checks its own status: callers run this on the left of ||, which suspends set -e.
# HOME may be absent from a plugin host's environment; then there is no ~/.cargo/env to source.
build_source() {
  [ -n "${HOME:-}" ] && [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  command -v cargo >/dev/null 2>&1 || return 127
  ( cd "$root" && cargo build --locked --release --quiet ) || return 1
  cp "$root/target/release/herd-gamepad" "$1"
}

# Exit 1 after build_source failed with status $1; $2 is the message for a missing cargo.
build_failed() {
  if [ "$1" = 127 ]; then echo "herd-gamepad: $2" >&2
  else echo "herd-gamepad: cargo build failed; the installed binary is unchanged" >&2
  fi
  exit 1
}

# Remove the staging file on any exit, and never exit 0 unless the binary was installed.
# /bin/sh (bash 3.2) sees $? as 0 in an EXIT trap after an unbound-variable error, so a run
# that died before the final mv would otherwise report success.
cleanup() {
  rc=$?
  rm -f "$staged"
  [ "$rc" != 0 ] || [ "$installed" = 1 ] || rc=1
  exit "$rc"
}

mkdir -p "$(dirname -- "$out")"
# A fresh inode avoids invalidating a running executable and macOS signature caches.
staged=$(mktemp "${out}.XXXXXX")
installed=0
trap cleanup EXIT HUP INT TERM
if [ "${HERD_GAMEPAD_BUILD:-}" = source ]; then
  build_source "$staged" || build_failed $? "cargo not found; install Rust from https://rustup.rs"
elif ! fetch_release "$staged"; then
  build_source "$staged" || build_failed $? "could not install the release binary, and cargo is not installed; check the network or install Rust from https://rustup.rs and retry"
fi
chmod +x "$staged"
mv -f "$staged" "$out"
installed=1
