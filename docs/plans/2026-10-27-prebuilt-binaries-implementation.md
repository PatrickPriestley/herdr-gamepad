# Prebuilt Release Binaries Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** `herdr plugin install` gets a verified prebuilt macOS binary from the GitHub release and only compiles when it cannot.

**Architecture:** `scripts/build.sh` tries the release asset for the manifest's version first (checksum-verified, fail-closed), then cargo; `HERD_GAMEPAD_BUILD=source` forces cargo. CI's two macOS jobs package their tested binaries and the existing `publish` job attaches them to the release after the unchanged Python publisher creates it. Design: `docs/plans/2026-10-27-prebuilt-binaries-design.md`.

**Tech Stack:** POSIX sh, curl, tar, shasum; GitHub Actions with SHA-pinned `actions/upload-artifact` and `actions/download-artifact`; `gh release upload`; actionlint for the workflow.

**Facts probed on 2026-10-27:** see the design's "Probed facts". Additionally: `curl -fsSL file:///dir/missing` exits non-zero, so a directory URL in `HERD_GAMEPAD_RELEASE_URL` stands in for the release in tests; `mktemp "${out}.XXXXXX"` creates the staging file next to the target as the current script does; `tar -xzf archive -C dir herd-gamepad` extracts the single member with bsdtar.

Work in the worktree `/private/tmp/herdr-gamepad-prebuilt` on branch `feat/prebuilt-binaries`. Commits are authored `PatrickPriestley <wppriestley@gmail.com>` with no trailer lines.

---

### Task 1: Build-script tests for the download path

**Files:**
- Modify: `tests/build.sh` (replace whole file)

**Step 1: Write the failing tests**

Replace `tests/build.sh` with:

```sh
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

# 7. An installed copy (binary present, no Cargo.toml) does nothing and never downloads.
f=$(fixture installed); release "$f"; rm "$f/Cargo.toml"
run "$f" "$base_path" "" || fail "installed: exit $?: $(cat "$f/log")"
[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "installed: an installed copy was rebuilt"

[ "$failures" = 0 ] && echo "build script: 8 cases passed" || { echo "build script: $failures failing" >&2; exit 1; }
```

**Step 2: Run it to verify it fails**

Run: `sh tests/build.sh; echo exit=$?`
Expected (verified against the current script): three FAIL lines from two cases, `prebuilt` (exit 1, cargo not found; then "expected the downloaded binary, got old binary") and `corrupt` (log does not mention the mismatch), then `build script: 4 failing` counting the empty-digest case below; the other cases already hold; exit 1.

**Step 3: Commit**

```bash
git add tests/build.sh
git commit -m "test: build script downloads a verified release binary before compiling"
```

---

### Task 2: The build script downloads first

**Files:**
- Modify: `scripts/build.sh` (replace whole file)

**Step 1: Write the implementation**

```sh
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
```

**Step 2: Run the tests to verify they pass**

Run: `sh tests/build.sh`
Expected: `build script: 8 cases passed`

**Step 3: Commit**

```bash
git add scripts/build.sh
git commit -m "build: install the verified release binary first, compile only as a fallback"
```

---

### Task 3: CI packages the tested binaries and attaches them to the release

**Files:**
- Modify: `.github/workflows/ci.yml`

**Step 1: Edit the matrix so each runner knows its target**

Replace

```yaml
      matrix:
        runner: [macos-15, macos-15-intel]
```

with

```yaml
      matrix:
        include:
          - runner: macos-15
            target: aarch64-apple-darwin
          - runner: macos-15-intel
            target: x86_64-apple-darwin
```

**Step 2: CI always builds from source, then packages**

Replace the step

```yaml
      - name: Build through the plugin entrypoint
        run: sh tests/build.sh && sh scripts/build.sh
```

with

```yaml
      - name: Build through the plugin entrypoint
        env:
          HERD_GAMEPAD_BUILD: source
        run: sh tests/build.sh && sh scripts/build.sh
```

and append after the "Smoke test the installed layout" step:

```yaml
      - name: Package the release binary and its checksum
        run: |
          asset=herd-gamepad-${{ matrix.target }}.tar.gz
          mkdir -p dist
          tar -czf "dist/$asset" -C target/release herd-gamepad
          (cd dist && shasum -a 256 "$asset" > "$asset.sha256")
          cat "dist/$asset.sha256"
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7
        with:
          name: herd-gamepad-${{ matrix.target }}
          path: dist/
          if-no-files-found: error
          retention-days: 7
```

**Step 3: The publish job attaches both assets after creating the release**

Insert before the "Publish verified source release" step:

```yaml
      - uses: actions/download-artifact@37930b1c2abaa49bbe596cd826c3c89aef350131 # v7
        with:
          pattern: herd-gamepad-*
          path: dist
          merge-multiple: true
```

and append after it:

```yaml
      - name: Attach the macOS binaries to the release
        env:
          GH_TOKEN: ${{ github.token }}
          TAG: ${{ github.ref_name }}
        run: |
          ls -l dist
          set -- dist/herd-gamepad-*.tar.gz; test "$#" -eq 2
          set -- dist/herd-gamepad-*.tar.gz.sha256; test "$#" -eq 2
          gh release upload "$TAG" dist/herd-gamepad-*.tar.gz dist/herd-gamepad-*.tar.gz.sha256 --repo "$GITHUB_REPOSITORY" --clobber
```

**Step 4: Lint**

Run: `actionlint .github/workflows/ci.yml && echo clean`
Expected: `clean`

**Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: package the tested macOS binaries and attach them to the release"
```

---

### Task 4: Development installer and docs

**Files:**
- Modify: `install.sh:30`, `README.md:14-21`, `CHANGELOG.md` (Unreleased)

**Step 1: install.sh builds from source**

Replace line 30

```bash
HERD_GAMEPAD_OUT="$tmp/bin/herd-gamepad" /bin/sh "$src/scripts/build.sh"
```

with

```bash
HERD_GAMEPAD_BUILD=source HERD_GAMEPAD_OUT="$tmp/bin/herd-gamepad" /bin/sh "$src/scripts/build.sh"
```

**Step 2: README requirements and install paragraph**

Replace

```markdown
- Git, a current stable Rust toolchain (`cargo`), and Xcode Command Line Tools.
```

with

```markdown
- Git. A Rust toolchain (`cargo`) is needed only when no prebuilt binary matches your Mac.
```

and replace

```markdown
This release builds from source; it does not include prebuilt binaries. Herdr runs the
build script during installation, using the committed Cargo.lock.
```

with

```markdown
Herdr runs the build script during installation. It downloads the release binary for your
Mac (Apple silicon or Intel), checks its SHA-256 against the checksum published with it, and
only compiles from source with the committed Cargo.lock when no binary is available.
Set `HERD_GAMEPAD_BUILD=source` to compile regardless.
```

**Step 3: CHANGELOG**

Add under `## Unreleased`, after the "Publish stable version tags" bullet:

```markdown
- Install the tested macOS release binary (checksum-verified) instead of compiling; cargo stays as the fallback and as the development path.
```

**Step 4: Check and commit**

Run: `./install.sh --dry-run` (prints the plan, changes nothing) and `grep -n HERD_GAMEPAD_BUILD install.sh`.
Expected: the dry-run line, and line 30 carries the variable.

```bash
git add install.sh README.md CHANGELOG.md
git commit -m "docs: prebuilt install path; dev installer builds from source"
```

---

### Task 5: Verify on the next tag (manual)

After merge, the next `vX.Y.Z` tag must show two `.tar.gz` and two `.sha256` assets on the release, and `herdr plugin install PatrickPriestley/herdr-gamepad --ref vX.Y.Z` on a Mac without cargo must install in seconds. Until a tagged release carries assets, every install falls back to the source build, as before.
