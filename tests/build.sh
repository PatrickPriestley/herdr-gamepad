#!/bin/sh
# scripts/build.sh: prebuilt first, cargo fallback, atomic replacement. Each case runs in a
# fixture with stubbed cargo and a file:// release directory; HOME points into the fixture so
# ~/.cargo/env cannot bring the real cargo in.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
case "$(uname -s) $(uname -m)" in
  "Darwin arm64")  target=aarch64-apple-darwin ;;
  "Darwin x86_64") target=x86_64-apple-darwin ;;
  *) echo "tests/build.sh: unsupported platform $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac
asset="herd-gamepad-$target.tar.gz"
failures=0
fail() { echo "FAIL: $1" >&2; failures=$((failures + 1)); }

# A fixture: the script, a source marker, an old installed binary, a cargo stub whose
# "build output" is a pre-made target/release/herd-gamepad, and an empty release directory.
fixture() {
  f="$work/$1"
  mkdir -p "$f/scripts" "$f/target/release" "$f/bin" "$f/tools" "$f/release"
  cp "$root/scripts/build.sh" "$f/scripts/build.sh"
  printf 'version = "9.9.9"\n' > "$f/herdr-plugin.toml"
  touch "$f/Cargo.toml"
  printf '#!/bin/sh\nexit 0\n' > "$f/tools/cargo"
  printf '#!/bin/sh\nprintf "new binary\\n"\n' > "$f/target/release/herd-gamepad"
  printf '#!/bin/sh\nprintf "old binary\\n"\n' > "$f/bin/herd-gamepad"
  chmod +x "$f/tools/cargo" "$f/target/release/herd-gamepad" "$f/bin/herd-gamepad"
  printf '%s' "$f"
}

# A release directory holding the asset and its checksum; $2 = "corrupt" writes a wrong digest.
release() {
  mkdir -p "$1/payload"
  printf '#!/bin/sh\nprintf "prebuilt binary\\n"\n' > "$1/payload/herd-gamepad"
  chmod +x "$1/payload/herd-gamepad"
  tar -czf "$1/release/$asset" -C "$1/payload" herd-gamepad
  if [ "${2:-}" = corrupt ]; then
    printf '%s  %s\n' "$(printf 'x%.0s' $(seq 64) | tr x 0)" "$asset" > "$1/release/$asset.sha256"
  else
    (cd "$1/release" && shasum -a 256 "$asset" > "$asset.sha256")
  fi
}

# Run the script in fixture $1 with PATH=$2 and extra env $3; stdout/stderr to $1/log.
run() {
  (cd "$1" && env -i HOME="$1" PATH="$2" HERD_GAMEPAD_RELEASE_URL="file://$1/release" $3 sh scripts/build.sh) > "$1/log" 2>&1
}
base_path=/usr/bin:/bin:/usr/sbin:/sbin

# 1. A source build replaces the inode, never the running executable's bytes.
f=$(fixture inode)
before=$(ls -i "$f/bin/herd-gamepad" | awk '{print $1}')
run "$f" "$f/tools:$base_path" HERD_GAMEPAD_BUILD=source || fail "inode: exit $?: $(cat "$f/log")"
after=$(ls -i "$f/bin/herd-gamepad" | awk '{print $1}')
[ "$before" != "$after" ] || fail "inode: build overwrote the installed inode"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "inode: expected the built binary"

# 2. With a release present and no cargo, the prebuilt binary is installed.
f=$(fixture prebuilt); release "$f"
run "$f" "$base_path" "" || fail "prebuilt: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "prebuilt binary" ] || fail "prebuilt: expected the downloaded binary, got $("$f/bin/herd-gamepad")"
[ -x "$f/bin/herd-gamepad" ] || fail "prebuilt: binary is not executable"
[ -z "$(ls "$f/bin" | grep -v '^herd-gamepad$')" ] || fail "prebuilt: staging file left behind: $(ls "$f/bin")"

# 3. A checksum mismatch never installs the download; cargo is used instead and the log says why.
f=$(fixture corrupt); release "$f" corrupt
run "$f" "$f/tools:$base_path" "" || fail "corrupt: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "corrupt: expected the cargo build, got $("$f/bin/herd-gamepad")"
grep -q "checksum mismatch" "$f/log" || fail "corrupt: log does not mention the mismatch: $(cat "$f/log")"

# 4. No release for this version: cargo builds it.
f=$(fixture missing)
run "$f" "$f/tools:$base_path" "" || fail "missing: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "missing: expected the cargo build"

# 5. HERD_GAMEPAD_BUILD=source never downloads, even when a release exists.
f=$(fixture source); release "$f"
run "$f" "$f/tools:$base_path" HERD_GAMEPAD_BUILD=source || fail "source: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "source: expected the cargo build over the release"

# 6. No release and no cargo: exit 1, old binary untouched, message names both remedies.
f=$(fixture nothing)
if run "$f" "$base_path" ""; then fail "nothing: expected a non-zero exit"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "nothing: old binary was replaced"
grep -q "rustup.rs" "$f/log" || fail "nothing: message does not say how to get cargo: $(cat "$f/log")"

# 8. An empty published digest is a mismatch, not a match against an empty computed one.
f=$(fixture emptydigest); release "$f"; : > "$f/release/$asset.sha256"
run "$f" "$f/tools:$base_path" "" || fail "emptydigest: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "emptydigest: the download was installed on an empty digest"
grep -q "checksum mismatch" "$f/log" || fail "emptydigest: log does not mention the mismatch: $(cat "$f/log")"

# 9. No checksum tool at all: an empty computed digest must not match an empty published one.
f=$(fixture nodigest); release "$f"; : > "$f/release/$asset.sha256"
printf '#!/bin/sh\nexit 1\n' > "$f/tools/shasum"; cp "$f/tools/shasum" "$f/tools/sha256sum"; chmod +x "$f/tools/shasum" "$f/tools/sha256sum"
run "$f" "$f/tools:$base_path" "" || fail "nodigest: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "nodigest: an unverifiable download was installed"
grep -q "checksum mismatch" "$f/log" || fail "nodigest: log does not mention the mismatch: $(cat "$f/log")"

# 10. A failing cargo build exits non-zero, leaves the old binary alone, keeps cargo's error in
# the log, and does not blame a missing cargo. A stale target/release/herd-gamepad exists.
f=$(fixture buildfail)
printf '#!/bin/sh\necho "error: could not compile" >&2\nexit 101\n' > "$f/tools/cargo"
if run "$f" "$f/tools:$base_path" HERD_GAMEPAD_BUILD=source; then fail "buildfail: expected a non-zero exit"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "buildfail: a stale target/release binary was installed"
grep -q "could not compile" "$f/log" || fail "buildfail: cargo's error is not in the log: $(cat "$f/log")"
if grep -q "rustup.rs" "$f/log"; then fail "buildfail: blamed a missing cargo although cargo ran: $(cat "$f/log")"; fi
[ -z "$(ls "$f/bin" | grep -v '^herd-gamepad$')" ] || fail "buildfail: staging file left behind: $(ls "$f/bin")"

# 11. No release and a failing cargo build: the same, on the download-fallback path.
f=$(fixture fallbackfail)
printf '#!/bin/sh\necho "error: could not compile" >&2\nexit 101\n' > "$f/tools/cargo"
if run "$f" "$f/tools:$base_path" ""; then fail "fallbackfail: expected a non-zero exit"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "fallbackfail: a stale target/release binary was installed"
grep -q "could not compile" "$f/log" || fail "fallbackfail: cargo's error is not in the log: $(cat "$f/log")"
if grep -q "cargo is not installed" "$f/log"; then fail "fallbackfail: blamed a missing cargo although cargo ran: $(cat "$f/log")"; fi

# 7. An installed copy (binary present, no Cargo.toml) does nothing and never downloads.
f=$(fixture installed); release "$f"; rm "$f/Cargo.toml"
run "$f" "$base_path" "" || fail "installed: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "installed: an installed copy was rebuilt"

[ "$failures" = 0 ] && echo "build script: 11 cases passed" || { echo "build script: $failures failing" >&2; exit 1; }
