#!/bin/sh
# scripts/build.sh: prebuilt first, cargo fallback, atomic replacement. The suite covers the
# download-first path and the curl timeouts, checksum verification failing closed, extraction
# failures, the cargo fallback and its messages, HERD_GAMEPAD_BUILD=source, the installed-copy
# guard, the environment (an unset HOME, an unknown build mode) and download cleanup.
# Each case runs in a fixture with stubbed cargo and a file:// release directory; HOME points
# into the fixture so ~/.cargo/env cannot bring the real cargo in, and TMPDIR points at the
# fixture's tmp/ so a leaked download directory is visible.
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
# "build output" is a pre-made target/release/herd-gamepad, an empty release directory and an
# empty tmp/ for the script's TMPDIR.
fixture() {
  f="$work/$1"
  mkdir -p "$f/scripts" "$f/target/release" "$f/bin" "$f/tools" "$f/release" "$f/tmp"
  cp "$root/scripts/build.sh" "$f/scripts/build.sh"
  printf 'version = "9.9.9"\n' > "$f/herdr-plugin.toml"
  touch "$f/Cargo.toml"
  printf '#!/bin/sh\nexit 0\n' > "$f/tools/cargo"
  printf '#!/bin/sh\nprintf "new binary\\n"\n' > "$f/target/release/herd-gamepad"
  printf '#!/bin/sh\nprintf "old binary\\n"\n' > "$f/bin/herd-gamepad"
  chmod +x "$f/tools/cargo" "$f/target/release/herd-gamepad" "$f/bin/herd-gamepad"
  printf '%s' "$f"
}

# A release directory holding the asset and its checksum. $2 = "corrupt" writes a wrong digest;
# "nomember" packs a tarball without herd-gamepad in it; "notgzip" publishes a plain file as
# the asset. Both of the latter carry a correct digest, so only extraction can reject them.
release() {
  mkdir -p "$1/payload"
  printf '#!/bin/sh\nprintf "prebuilt binary\\n"\n' > "$1/payload/herd-gamepad"
  chmod +x "$1/payload/herd-gamepad"
  case "${2:-}" in
    nomember) : > "$1/payload/other"; tar -czf "$1/release/$asset" -C "$1/payload" other ;;
    notgzip)  printf 'not a tarball\n' > "$1/release/$asset" ;;
    *)        tar -czf "$1/release/$asset" -C "$1/payload" herd-gamepad ;;
  esac
  if [ "${2:-}" = corrupt ]; then
    printf '%s  %s\n' "$(printf 'x%.0s' $(seq 64) | tr x 0)" "$asset" > "$1/release/$asset.sha256"
  else
    (cd "$1/release" && shasum -a 256 "$asset" > "$asset.sha256")
  fi
}

# A stub $2 in fixture $1's tools that records that it ran in $1/$2-ran, then fails loudly.
# Use it to prove a tool was never called.
forbid() {
  printf '#!/bin/sh\n: > "%s/%s-ran"\necho "%s stub must not run" >&2\nexit 1\n' "$1" "$2" "$2" > "$1/tools/$2"
  chmod +x "$1/tools/$2"
}

# The script's download directories are created under TMPDIR=$1/tmp; every run must leave it
# empty, whichever way the run ended.
assert_clean_tmp() {
  [ -z "$(ls -A "$1/tmp")" ] || fail "$(basename "$1"): download directory left in TMPDIR: $(ls -A "$1/tmp")"
}

# Run the script in fixture $1 with PATH=$2 and extra env $3; stdout/stderr to $1/log. HOME is
# the fixture unless $4 is "nohome", which leaves HOME out of the environment entirely.
# Returns the script's status after checking that TMPDIR was left clean.
run() {
  if [ "${4:-}" = nohome ]; then home=; else home="HOME=$1"; fi
  (cd "$1" && env -i $home TMPDIR="$1/tmp" PATH="$2" HERD_GAMEPAD_RELEASE_URL="file://$1/release" $3 sh scripts/build.sh) > "$1/log" 2>&1
  rc=$?
  assert_clean_tmp "$1"
  return $rc
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

# 3. Downloads carry connection and total timeouts, so a stalled server falls back to cargo
# instead of hanging the install, and land under TMPDIR, where assert_clean_tmp can see them.
# A curl stub records its arguments and fails like a 404.
f=$(fixture curlargs); release "$f"
printf '#!/bin/sh\necho "$*" >> "%s/curl-args"\nexit 22\n' "$f" > "$f/tools/curl"; chmod +x "$f/tools/curl"
run "$f" "$f/tools:$base_path" "" || fail "curlargs: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "curlargs: expected the cargo fallback"
grep -q -- "--connect-timeout 15" "$f/curl-args" 2>/dev/null || fail "curlargs: curl ran without a connect timeout: $(cat "$f/curl-args" 2>&1)"
grep -q -- "--max-time 300" "$f/curl-args" 2>/dev/null || fail "curlargs: curl ran without a total timeout: $(cat "$f/curl-args" 2>&1)"
grep -q -- "-o $f/tmp/" "$f/curl-args" 2>/dev/null || fail "curlargs: download directory is not under TMPDIR, so assert_clean_tmp observes nothing: $(cat "$f/curl-args" 2>&1)"

# 4. A checksum mismatch never installs the download; cargo is used instead and the log says why.
f=$(fixture corrupt); release "$f" corrupt
run "$f" "$f/tools:$base_path" "" || fail "corrupt: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "corrupt: expected the cargo build, got $("$f/bin/herd-gamepad")"
grep -q "checksum mismatch" "$f/log" || fail "corrupt: log does not mention the mismatch: $(cat "$f/log")"

# 5. No release for this version: cargo builds it.
f=$(fixture missing)
run "$f" "$f/tools:$base_path" "" || fail "missing: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "missing: expected the cargo build"

# 6. HERD_GAMEPAD_BUILD=source never downloads, even when a release exists.
f=$(fixture source); release "$f"
run "$f" "$f/tools:$base_path" HERD_GAMEPAD_BUILD=source || fail "source: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "source: expected the cargo build over the release"

# 7. A source build with no HOME in the environment at all (a minimal environment from a
# plugin host) still builds, rather than dying on an unbound variable with exit 0.
f=$(fixture nohome)
run "$f" "$f/tools:$base_path" HERD_GAMEPAD_BUILD=source nohome || fail "nohome: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "nohome: expected the built binary, got '$("$f/bin/herd-gamepad")': $(cat "$f/log")"
if grep -q "unbound variable" "$f/log"; then fail "nohome: $(cat "$f/log")"; fi

# 8. A ~/.cargo/env the script sources (a user-owned file) that trips set -u aborts the shell
# before any exit statement; the run must still exit non-zero with the staging file removed,
# not report success: /bin/sh hands the EXIT trap $?=0 after an unbound-variable error.
f=$(fixture badenv); mkdir -p "$f/.cargo"; printf ': "$HERD_GAMEPAD_TEST_UNSET"\n' > "$f/.cargo/env"
if run "$f" "$f/tools:$base_path" HERD_GAMEPAD_BUILD=source; then fail "badenv: expected a non-zero exit: $(cat "$f/log")"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "badenv: old binary was replaced"
[ -z "$(ls "$f/bin" | grep -v '^herd-gamepad$')" ] || fail "badenv: staging file left behind: $(ls "$f/bin")"

# 8b. The success sentinel must not be inheritable: with installed=1 exported by the caller, the
# same shell error must still exit non-zero.
f=$(fixture inherited); mkdir -p "$f/.cargo"; printf ': "$HERD_GAMEPAD_TEST_UNSET"\n' > "$f/.cargo/env"
if run "$f" "$f/tools:$base_path" "HERD_GAMEPAD_BUILD=source installed=1"; then fail "inherited: an exported installed=1 masked the failure: $(cat "$f/log")"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "inherited: old binary was replaced"

# 9. An unknown HERD_GAMEPAD_BUILD value is rejected before anything is downloaded or built:
# exit 1, old binary untouched, a message naming the value, and neither curl nor cargo ran.
f=$(fixture unknownmode); release "$f"; forbid "$f" curl; forbid "$f" cargo
if run "$f" "$f/tools:$base_path" HERD_GAMEPAD_BUILD=Source; then fail "unknownmode: expected a non-zero exit: $(cat "$f/log")"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "unknownmode: old binary was replaced"
grep -q 'unknown HERD_GAMEPAD_BUILD value "Source"; use source or leave it unset' "$f/log" || fail "unknownmode: message missing: $(cat "$f/log")"
[ ! -e "$f/curl-ran" ] || fail "unknownmode: a download was attempted"
[ ! -e "$f/cargo-ran" ] || fail "unknownmode: cargo ran"
[ -z "$(ls "$f/bin" | grep -v '^herd-gamepad$')" ] || fail "unknownmode: staging file left behind: $(ls "$f/bin")"

# 10. No release and no cargo: exit 1, old binary untouched, message names both remedies and
# says the release binary could not be installed.
f=$(fixture nothing)
if run "$f" "$base_path" ""; then fail "nothing: expected a non-zero exit"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "nothing: old binary was replaced"
grep -q "rustup.rs" "$f/log" || fail "nothing: message does not say how to get cargo: $(cat "$f/log")"
grep -q "could not install the release binary" "$f/log" || fail "nothing: message does not name the failed install: $(cat "$f/log")"

# 11. An empty published digest is a mismatch, not a match against an empty computed one.
f=$(fixture emptydigest); release "$f"; : > "$f/release/$asset.sha256"
run "$f" "$f/tools:$base_path" "" || fail "emptydigest: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "emptydigest: the download was installed on an empty digest"
grep -q "checksum mismatch" "$f/log" || fail "emptydigest: log does not mention the mismatch: $(cat "$f/log")"

# 12. No checksum tool at all: an empty computed digest must not match an empty published one.
f=$(fixture nodigest); release "$f"; : > "$f/release/$asset.sha256"
printf '#!/bin/sh\nexit 1\n' > "$f/tools/shasum"; cp "$f/tools/shasum" "$f/tools/sha256sum"; chmod +x "$f/tools/shasum" "$f/tools/sha256sum"
run "$f" "$f/tools:$base_path" "" || fail "nodigest: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "nodigest: an unverifiable download was installed"
grep -q "checksum mismatch" "$f/log" || fail "nodigest: log does not mention the mismatch: $(cat "$f/log")"

# 13. A failing cargo build exits non-zero, leaves the old binary alone, removes the staging
# file, keeps cargo's error in the log, and does not blame a missing cargo. A stale
# target/release/herd-gamepad exists.
f=$(fixture buildfail)
printf '#!/bin/sh\necho "error: could not compile" >&2\nexit 101\n' > "$f/tools/cargo"
if run "$f" "$f/tools:$base_path" HERD_GAMEPAD_BUILD=source; then fail "buildfail: expected a non-zero exit"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "buildfail: a stale target/release binary was installed"
grep -q "could not compile" "$f/log" || fail "buildfail: cargo's error is not in the log: $(cat "$f/log")"
if grep -q "rustup.rs" "$f/log"; then fail "buildfail: blamed a missing cargo although cargo ran: $(cat "$f/log")"; fi
[ -z "$(ls "$f/bin" | grep -v '^herd-gamepad$')" ] || fail "buildfail: staging file left behind: $(ls "$f/bin")"

# 14. No release and a failing cargo build: the same, on the download-fallback path.
f=$(fixture fallbackfail)
printf '#!/bin/sh\necho "error: could not compile" >&2\nexit 101\n' > "$f/tools/cargo"
if run "$f" "$f/tools:$base_path" ""; then fail "fallbackfail: expected a non-zero exit"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "fallbackfail: a stale target/release binary was installed"
grep -q "could not compile" "$f/log" || fail "fallbackfail: cargo's error is not in the log: $(cat "$f/log")"
if grep -q "cargo is not installed" "$f/log"; then fail "fallbackfail: blamed a missing cargo although cargo ran: $(cat "$f/log")"; fi
[ -z "$(ls "$f/bin" | grep -v '^herd-gamepad$')" ] || fail "fallbackfail: staging file left behind: $(ls "$f/bin")"

# 15. A verified tarball without a herd-gamepad member is not installed: cargo builds instead,
# and the log names the asset that lacked the binary.
f=$(fixture nomember); release "$f" nomember
run "$f" "$f/tools:$base_path" "" || fail "nomember: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "nomember: expected the cargo build, got '$("$f/bin/herd-gamepad" 2>&1)'"
[ -z "$(ls "$f/bin" | grep -v '^herd-gamepad$')" ] || fail "nomember: staging file left behind: $(ls "$f/bin")"
grep -q "release asset $asset did not contain a usable herd-gamepad binary" "$f/log" || fail "nomember: log does not name the unusable asset: $(cat "$f/log")"

# 16. The same tarball with no cargo: exit non-zero, old binary untouched, no empty binary,
# and the message says the release binary could not be installed (not "downloaded").
f=$(fixture nomembernocargo); release "$f" nomember
if run "$f" "$base_path" ""; then fail "nomembernocargo: expected a non-zero exit: $(cat "$f/log")"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "nomembernocargo: old binary was replaced by '$("$f/bin/herd-gamepad" 2>&1)'"
grep -q "rustup.rs" "$f/log" || fail "nomembernocargo: message does not say how to get cargo: $(cat "$f/log")"
grep -q "release asset $asset did not contain a usable herd-gamepad binary" "$f/log" || fail "nomembernocargo: log does not name the unusable asset: $(cat "$f/log")"
grep -q "could not install the release binary" "$f/log" || fail "nomembernocargo: message does not name the failed install: $(cat "$f/log")"
if grep -q "could not download" "$f/log"; then fail "nomembernocargo: message blames the download although it succeeded: $(cat "$f/log")"; fi

# 17. A verified asset that is not a gzip tarball at all: the same, with no cargo.
f=$(fixture notgzip); release "$f" notgzip
if run "$f" "$base_path" ""; then fail "notgzip: expected a non-zero exit: $(cat "$f/log")"; fi
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "notgzip: old binary was replaced by '$("$f/bin/herd-gamepad" 2>&1)'"
[ -z "$(ls "$f/bin" | grep -v '^herd-gamepad$')" ] || fail "notgzip: staging file left behind: $(ls "$f/bin")"
grep -q "release asset $asset did not contain a usable herd-gamepad binary" "$f/log" || fail "notgzip: log does not name the unusable asset: $(cat "$f/log")"

# 18. An installed copy (binary present, no Cargo.toml) does nothing and never downloads.
f=$(fixture installed); release "$f"; rm "$f/Cargo.toml"; forbid "$f" curl
run "$f" "$f/tools:$base_path" "" || fail "installed: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "installed: an installed copy was rebuilt"
[ ! -e "$f/curl-ran" ] || fail "installed: a download was attempted"

[ "$failures" = 0 ] && echo "build script: 19 cases passed" || { echo "build script: $failures failing" >&2; exit 1; }
