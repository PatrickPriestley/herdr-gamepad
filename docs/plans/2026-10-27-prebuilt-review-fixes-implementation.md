# PR 5: bind prebuilt binaries to their source checkout

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Correct PR #5 so prebuilt installation cannot silently substitute an older release binary for the requested checkout, and close the verified checksum/test/documentation gaps.

**Architecture:** The existing checked tarball also contains `herd-gamepad.commit`, stamped from the tested CI checkout. Immediately after download and extraction, the installer resolves the source root and HEAD and requires a clean Git status. It accepts the binary only when the embedded commit equals that HEAD; otherwise the existing atomic source-build fallback applies. Checksum tool output is captured before parsing so a failing tool cannot be hidden by `cut`.

**Tech stack:** POSIX shell, Git, existing curl/tar/checksum tools, Python standard-library contract test, existing macOS CI.

**Baseline:** `ef7ebb44537632dfdf40b4e533fa92972720995a` (PR #5 merged with released main), plus design-only commit `41883cc`.
**Design:** Review-correction appendix in `docs/plans/2026-10-27-prebuilt-binaries-design.md`.
**Scope:** Update PR #5 only. No merge, new release, retrospective release assets, or installed-plugin replacement is part of this fix.

## Probed facts and decisions

- The old installer was reproduced installing `released-old` for a newer same-version checkout despite cargo being available. The local fixture in `/tmp/probe-pr5-ref.py` fails its positive expected-source assertion.
- Herdr-style `git fetch --depth 1 origin TAG_OR_SHA; checkout --detach FETCH_HEAD` creates no local tag in either case. A local exact-tag check would reject valid tagged installs. Probe: `/tmp/pr5-git-probe-mkvg888s`.
- A tar archive can carry both the binary and tested commit without another network request. The archive checksum covers both.
- Both original checksum pipelines accept a tool that prints the correct digest and exits 7: `cut` hides the failure. Capture and check tool status before parsing.
- An early checkout check is insufficient: a curl fixture changed or committed source during download and the old cached identity still accepted the binary. `/tmp/pr5-download-change-red.log` demonstrates both failures. Validate the checkout after download/extraction instead. This closes the long download window; it does not claim an atomic lock against concurrent source edits after the final check.
- `/usr/bin/git` can create `xcrun_db` under TMPDIR. Fixture cleanup ignores only this tool-owned cache and continues to check installer download cleanup.
- The existing installed v0.2.0 checkout resolves to its own Git root and has clean porcelain status. Herdr runs the build before saving registry metadata.
- New binaries must match a clean checkout root, HEAD, and extracted commit. Source archives, nested directories, dirty tracked/untracked files, Git failures, missing/invalid metadata, extraction/read/copy failures all use the existing source fallback. Older releases through v0.2.0 stay source-built.

## Task 1: lock down regressions

Files: `tests/build.sh`, `tests/test_release_package.py`.

Apply the exact test patch below. Run:

```sh
sh tests/build.sh
python3 tests/test_release_package.py
```

The baseline installer produces 22 failing assertions across stale/missing provenance, dirty/newer checkouts, failed Git/checksum/metadata commands, and changes during download. The CI package test fails its positive requirement that the archive include the tested commit. The checksum-tool absence and fallback scenarios already behaved correctly in the old implementation; their new coverage is proven by deleting those guards and watching the corresponding positive assertions fail, not falsely claimed as baseline production bugs.

Commit: `test: require prebuilt binaries to match their checkout`.

```diff
--- a/tests/build.sh
+++ b/tests/build.sh
@@ -32,6 +32,10 @@
   printf '#!/bin/sh\nprintf "new binary\\n"\n' > "$f/target/release/herd-gamepad"
   printf '#!/bin/sh\nprintf "old binary\\n"\n' > "$f/bin/herd-gamepad"
   chmod +x "$f/tools/cargo" "$f/target/release/herd-gamepad" "$f/bin/herd-gamepad"
+  printf '/bin/\n/target/\n/tools/\n/release/\n/payload/\n/tmp/\n/.cargo/\n/log\n/*-ran\n/curl-args\n/minimal/\n' > "$f/.gitignore"
+  git -C "$f" init -q
+  git -C "$f" add .gitignore scripts/build.sh Cargo.toml herdr-plugin.toml
+  git -C "$f" -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=Test -c user.email=test@example.invalid commit -qm fixture
   printf '%s' "$f"
 }
 
@@ -42,10 +46,11 @@
   mkdir -p "$1/payload"
   printf '#!/bin/sh\nprintf "prebuilt binary\\n"\n' > "$1/payload/herd-gamepad"
   chmod +x "$1/payload/herd-gamepad"
+  git -C "$1" rev-parse HEAD > "$1/payload/herd-gamepad.commit"
   case "${2:-}" in
     nomember) : > "$1/payload/other"; tar -czf "$1/release/$asset" -C "$1/payload" other ;;
     notgzip)  printf 'not a tarball\n' > "$1/release/$asset" ;;
-    *)        tar -czf "$1/release/$asset" -C "$1/payload" herd-gamepad ;;
+    *)        tar -czf "$1/release/$asset" -C "$1/payload" herd-gamepad herd-gamepad.commit ;;
   esac
   if [ "${2:-}" = corrupt ]; then
     printf '%s  %s\n' "$(printf 'x%.0s' $(seq 64) | tr x 0)" "$asset" > "$1/release/$asset.sha256"
@@ -64,7 +69,8 @@
 # The script's download directories are created under TMPDIR=$1/tmp; every run must leave it
 # empty, whichever way the run ended.
 assert_clean_tmp() {
-  [ -z "$(ls -A "$1/tmp")" ] || fail "$(basename "$1"): download directory left in TMPDIR: $(ls -A "$1/tmp")"
+  # Apple's /usr/bin/git launcher may create xcrun_db in TMPDIR; that cache is not ours.
+  [ -z "$(ls -A "$1/tmp" | grep -v '^xcrun_db$')" ] || fail "$(basename "$1"): download directory left in TMPDIR: $(ls -A "$1/tmp")"
 }
 
 # Run the script in fixture $1 with PATH=$2 and extra env $3; stdout/stderr to $1/log. HOME is
@@ -72,10 +78,17 @@
 # Returns the script's status after checking that TMPDIR was left clean.
 run() {
   if [ "${4:-}" = nohome ]; then home=; else home="HOME=$1"; fi
-  (cd "$1" && env -i $home TMPDIR="$1/tmp" PATH="$2" HERD_GAMEPAD_RELEASE_URL="file://$1/release" $3 sh scripts/build.sh) > "$1/log" 2>&1
+  (cd "$1" && env -i $home TMPDIR="$1/tmp" PATH="$2" HERD_GAMEPAD_RELEASE_URL="file://$1/release" $3 sh "${5:-scripts/build.sh}") > "$1/log" 2>&1
   rc=$?
   assert_clean_tmp "$1"
   return $rc
+}
+# Only commands the installer needs; checksum tools are deliberately absent.
+minimal_path() {
+  mkdir -p "$1/minimal"
+  for path_tool in sh dirname mkdir mktemp uname git sed curl cut tar cp rm chmod mv cat; do
+    ln -s "$(command -v "$path_tool")" "$1/minimal/$path_tool"
+  done
 }
 base_path=/usr/bin:/bin:/usr/sbin:/sbin
 
@@ -166,12 +179,16 @@
 [ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "emptydigest: the download was installed on an empty digest"
 grep -q "checksum mismatch" "$f/log" || fail "emptydigest: log does not mention the mismatch: $(cat "$f/log")"
 
-# 12. No checksum tool at all: an empty computed digest must not match an empty published one.
-f=$(fixture nodigest); release "$f"; : > "$f/release/$asset.sha256"
-printf '#!/bin/sh\nexit 1\n' > "$f/tools/shasum"; cp "$f/tools/shasum" "$f/tools/sha256sum"; chmod +x "$f/tools/shasum" "$f/tools/sha256sum"
-run "$f" "$f/tools:$base_path" "" || fail "nodigest: exit $?: $(cat "$f/log")"
+# 12. Neither checksum tool exists on PATH: the download cannot be verified.
+f=$(fixture nodigest); release "$f"
+minimal_path "$f"
+if env PATH="$f/minimal" /bin/sh -c 'command -v shasum || command -v sha256sum'; then fail "nodigest: checksum tools unexpectedly present"; fi
+run "$f" "$f/tools:$f/minimal" "" || fail "nodigest: exit $?: $(cat "$f/log")"
 [ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "nodigest: an unverifiable download was installed"
 grep -q "checksum mismatch" "$f/log" || fail "nodigest: log does not mention the mismatch: $(cat "$f/log")"
+# Also exercise the checksum helper's failure contract, rather than only its empty output.
+sed -n '/^sha256_of() {/,/^}/p' "$f/scripts/build.sh" > "$f/tools/checksum-function.sh"
+if env PATH="$f/minimal" /bin/sh -c '. "$1"; sha256_of "$2"' probe "$f/tools/checksum-function.sh" "$f/release/$asset"; then fail "nodigest: helper reported success without a checksum tool"; fi
 
 # 13. A failing cargo build exits non-zero, leaves the old binary alone, removes the staging
 # file, keeps cargo's error in the log, and does not blame a missing cargo. A stale
@@ -224,4 +241,148 @@
 [ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "installed: an installed copy was rebuilt"
 [ ! -e "$f/curl-ran" ] || fail "installed: a download was attempted"
 
-[ "$failures" = 0 ] && echo "build script: 19 cases passed" || { echo "build script: $failures failing" >&2; exit 1; }
+# 19. The sha256sum fallback succeeds when shasum really is absent.
+f=$(fixture sha256sum); release "$f"; minimal_path "$f"
+printf '#!/bin/sh\n: > "%s/sha256sum-ran"\nexec "%s" -a 256 "$@"\n' "$f" "$(command -v shasum)" > "$f/minimal/sha256sum"; chmod +x "$f/minimal/sha256sum"
+run "$f" "$f/minimal" "" || fail "sha256sum: exit $?: $(cat "$f/log")"
+[ "$("$f/bin/herd-gamepad")" = "prebuilt binary" ] || fail "sha256sum: expected verified prebuilt binary"
+[ -f "$f/sha256sum-ran" ] || fail "sha256sum: fallback tool was not called"
+
+# 20. A newer checkout with the same manifest version must build its own sources.
+f=$(fixture newer); release "$f"
+printf 'newer source\n' > "$f/Cargo.toml"
+git -C "$f" add Cargo.toml
+git -C "$f" -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=Test -c user.email=test@example.invalid commit -qm newer
+run "$f" "$f/tools:$base_path" "" || fail "newer: exit $?: $(cat "$f/log")"
+[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "newer: installed an older binary for a newer checkout"
+grep -q 'source commit mismatch' "$f/log" || fail "newer: missing provenance diagnostic"
+
+# 21. Without cargo that same mismatch must fail, preserving the installed binary.
+f=$(fixture newer-nocargo); release "$f"
+printf 'newer source\n' > "$f/Cargo.toml"; git -C "$f" add Cargo.toml
+git -C "$f" -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=Test -c user.email=test@example.invalid commit -qm newer
+if run "$f" "$base_path" ""; then fail "newer-nocargo: accepted an older binary"; fi
+[ "$("$f/bin/herd-gamepad")" = "old binary" ] || fail "newer-nocargo: changed the installed binary"
+
+# 22-23. Local tracked edits and untracked source files both require a source build.
+for kind in tracked untracked; do
+  f=$(fixture "$kind"); release "$f"
+  if [ "$kind" = tracked ]; then printf 'local edit\n' > "$f/Cargo.toml"
+  else printf 'new source\n' > "$f/new-source.rs"; fi
+  run "$f" "$f/tools:$base_path" "" || fail "$kind: exit $?: $(cat "$f/log")"
+  [ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "$kind: local sources were ignored"
+  grep -q 'checkout is not clean' "$f/log" || fail "$kind: missing dirty-source diagnostic"
+done
+
+# 24. Source archives without Git metadata cannot prove commit provenance.
+f=$(fixture no-git); release "$f"; rm -rf "$f/.git"
+run "$f" "$f/tools:$base_path" "" || fail "no-git: exit $?: $(cat "$f/log")"
+[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "no-git: unverified prebuilt installed"
+
+# 25. A nested source directory may not borrow its parent's otherwise clean Git identity.
+parent="$work/nested-parent"; mkdir -p "$parent"; printf '*\n' > "$parent/.gitignore"
+git -C "$parent" init -q; git -C "$parent" add -f .gitignore
+git -C "$parent" -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=Test -c user.email=test@example.invalid commit -qm parent
+f=$(fixture nested-parent/child); rm -rf "$f/.git"; release "$f"
+run "$f" "$f/tools:$base_path" "" || fail "nested: exit $?: $(cat "$f/log")"
+[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "nested: borrowed parent's provenance"
+
+# 26-27. A legacy or malformed provenance file is not sufficient, even with a valid checksum.
+for kind in missing-commit malformed-commit; do
+  f=$(fixture "$kind"); release "$f"
+  if [ "$kind" = missing-commit ]; then tar -czf "$f/release/$asset" -C "$f/payload" herd-gamepad
+  else printf 'not-a-commit\n' > "$f/payload/herd-gamepad.commit"; tar -czf "$f/release/$asset" -C "$f/payload" herd-gamepad herd-gamepad.commit; fi
+  (cd "$f/release" && shasum -a 256 "$asset" > "$asset.sha256")
+  run "$f" "$f/tools:$base_path" "" || fail "$kind: exit $?: $(cat "$f/log")"
+  [ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "$kind: unverified source identity installed"
+done
+
+# 28-29. Tool failures cannot be masked by a pipeline even if the output is a valid digest.
+for tool in shasum sha256sum; do
+  f=$(fixture "failed-$tool"); release "$f"; minimal_path "$f"
+  printf '#!/bin/sh\n/bin/cat "%s/release/%s.sha256"\nexit 7\n' "$f" "$asset" > "$f/tools/$tool"; chmod +x "$f/tools/$tool"
+  run "$f" "$f/tools:$f/minimal" "" || fail "$tool-failure: exit $?: $(cat "$f/log")"
+  [ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "$tool-failure: accepted output from a failed checksum command"
+  sed -n '/^sha256_of() {/,/^}/p' "$f/scripts/build.sh" > "$f/tools/checksum-function.sh"
+  if env PATH="$f/tools:$f/minimal" /bin/sh -c '. "$1"; sha256_of "$2"' probe "$f/tools/checksum-function.sh" "$f/release/$asset"; then fail "$tool-failure: helper masked checksum status"; fi
+done
+
+# 30-32. Every Git command's failure is checked, even if it printed plausible output.
+real_git=$(command -v git)
+for operation in root head status; do
+  f=$(fixture "failed-git-$operation"); release "$f"
+  cat > "$f/tools/git" <<GIT
+#!/bin/sh
+case "\$*" in
+  *'--show-toplevel') op=root ;;
+  *'--verify HEAD') op=head ;;
+  *'status --porcelain'*) op=status ;;
+  *) op=other ;;
+esac
+"$real_git" "\$@"
+rc=\$?
+[ "\$op" != "$operation" ] || exit 1
+exit \$rc
+GIT
+  chmod +x "$f/tools/git"
+  run "$f" "$f/tools:$base_path" "" || fail "git-$operation: exit $?: $(cat "$f/log")"
+  [ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "git-$operation: accepted failed provenance command"
+done
+
+# 33. A symlinked path to a clean checkout still qualifies for its own prebuilt binary.
+f=$(fixture symlink); release "$f"; ln -s "$f" "$work/symlink-path"
+run "$f" "$base_path" "" "" "$work/symlink-path/scripts/build.sh" || fail "symlink: exit $?: $(cat "$f/log")"
+[ "$("$f/bin/herd-gamepad")" = "prebuilt binary" ] || fail "symlink: canonical checkout was rejected"
+
+# 34. A failed provenance read is rejected even if it printed the expected commit.
+f=$(fixture failed-commit-read); release "$f"
+printf '#!/bin/sh\n/bin/cat "$@"\nexit 1\n' > "$f/tools/cat"; chmod +x "$f/tools/cat"
+run "$f" "$f/tools:$base_path" "" || fail "commit-read: exit $?: $(cat "$f/log")"
+[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "commit-read: accepted a failed metadata read"
+
+# 35. A failed staging copy must fall back, not install partial bytes.
+f=$(fixture failed-copy); release "$f"
+cat > "$f/tools/cp" <<COPY
+#!/bin/sh
+case "\$1" in
+  "$f/tmp/"*/herd-gamepad) printf partial > "\$2"; exit 1 ;;
+esac
+exec /bin/cp "\$@"
+COPY
+chmod +x "$f/tools/cp"
+run "$f" "$f/tools:$base_path" "" || fail "copy: exit $?: $(cat "$f/log")"
+[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "copy: installed a failed partial copy"
+
+# 36. A tar failure cannot be hidden by usable-looking partial extraction output.
+f=$(fixture failed-extract); release "$f"
+printf '#!/bin/sh\n"%s" "$@"\nexit 1\n' "$(command -v tar)" > "$f/tools/tar"; chmod +x "$f/tools/tar"
+run "$f" "$f/tools:$base_path" "" || fail "extract: exit $?: $(cat "$f/log")"
+[ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "extract: installed output from a failed extraction"
+
+# 37-38. A checkout changed during download must be checked again before installation.
+for change in commit dirty; do
+  f=$(fixture "download-$change"); release "$f"
+  cat > "$f/tools/curl" <<CURL
+#!/bin/sh
+if [ ! -e "$f/curl-ran" ]; then
+  : > "$f/curl-ran"
+  printf 'changed during download\\n' > "$f/Cargo.toml"
+  if [ "$change" = commit ]; then
+    "$real_git" -C "$f" add Cargo.toml
+    "$real_git" -C "$f" -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=Test -c user.email=test@example.invalid commit -qm changed
+  fi
+fi
+exec "$(command -v curl)" "\$@"
+CURL
+  chmod +x "$f/tools/curl"
+  run "$f" "$f/tools:$base_path" "" || fail "download-$change: exit $?: $(cat "$f/log")"
+  [ "$("$f/bin/herd-gamepad")" = "new binary" ] || fail "download-$change: installed prebuilt for changed checkout"
+done
+
+# 39. shasum alone is also sufficient; do not accidentally require sha256sum.
+f=$(fixture shasum-only); release "$f"; minimal_path "$f"
+ln -s "$(command -v shasum)" "$f/minimal/shasum"
+run "$f" "$f/minimal" "" || fail "shasum-only: exit $?: $(cat "$f/log")"
+[ "$("$f/bin/herd-gamepad")" = "prebuilt binary" ] || fail "shasum-only: expected verified prebuilt binary"
+
+[ "$failures" = 0 ] && echo "build script: 40 cases passed" || { echo "build script: $failures failing" >&2; exit 1; }
--- /dev/null
+++ b/tests/test_release_package.py
@@ -0,0 +1,62 @@
+"""Exercise the actual CI packaging block and its commit/checksum contract."""
+import hashlib
+from pathlib import Path
+import subprocess
+import tarfile
+import tempfile
+import unittest
+
+
+class PackageTests(unittest.TestCase):
+    def test_ci_packages_tested_head_for_both_macos_targets(self):
+        root = Path(__file__).resolve().parents[1]
+        workflow = (root / ".github/workflows/ci.yml").read_text()
+        step = workflow.split(
+            "      - name: Package the release binary and its checksum\n", 1
+        )[1].split("      - uses:", 1)[0]
+        run_block = step.split("        run: |\n", 1)[1]
+        script = "\n".join(line[10:] for line in run_block.splitlines())
+        for target in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
+            with self.subTest(target=target), tempfile.TemporaryDirectory() as name:
+                checkout = Path(name)
+                release = checkout / "target/release"
+                release.mkdir(parents=True)
+                (release / "herd-gamepad").write_bytes(b"tested binary\n")
+                # A stale stamp makes omission of the stamping command observable.
+                (release / "herd-gamepad.commit").write_text("stale\n")
+                (checkout / "source").write_text("tested source\n")
+
+                def run(argv):
+                    return subprocess.run(
+                        argv, cwd=checkout, check=True, text=True, capture_output=True
+                    ).stdout
+
+                run(["git", "init", "-q"])
+                run(["git", "add", "source"])
+                run([
+                    "git", "-c", "core.hooksPath=/dev/null",
+                    "-c", "commit.gpgsign=false", "-c", "user.name=Test",
+                    "-c", "user.email=test@example.invalid", "commit", "-qm", "tested",
+                ])
+                head = run(["git", "rev-parse", "HEAD"])
+                run([
+                    "/bin/sh", "-eu", "-c",
+                    script.replace("${{ matrix.target }}", target),
+                ])
+                asset = checkout / f"dist/herd-gamepad-{target}.tar.gz"
+                with tarfile.open(asset, "r:gz") as archive:
+                    self.assertIn("herd-gamepad.commit", archive.getnames())
+                    self.assertEqual(
+                        archive.extractfile("herd-gamepad.commit").read().decode(), head
+                    )
+                    self.assertEqual(
+                        archive.extractfile("herd-gamepad").read(), b"tested binary\n"
+                    )
+                self.assertEqual(
+                    asset.with_suffix(asset.suffix + ".sha256").read_text().split()[0],
+                    hashlib.sha256(asset.read_bytes()).hexdigest(),
+                )
+
+
+if __name__ == "__main__":
+    unittest.main()
```

## Task 2: apply the verified installer and packaging fix

Files: `scripts/build.sh`, `.github/workflows/ci.yml`, `README.md`, `CHANGELOG.md`.

Apply the exact code patch below. Preserve the tested root/HEAD/status failure checks and the late checkout validation. Keep staging and the installed-copy guard unchanged. Move the unreleased prebuilt feature out of the already-published 0.2.0 changelog section, and explain the archive/commit/checksum contract and older source-only releases.

```diff
--- a/scripts/build.sh
+++ b/scripts/build.sh
@@ -1,13 +1,13 @@
 #!/bin/sh
 # Put the herd-gamepad binary at bin/herd-gamepad (or $HERD_GAMEPAD_OUT). An installed copy
 # that already has its binary and ships no sources does nothing.
-# Order: the GitHub release binary for this platform and the manifest's version, checked
-# against its SHA-256; then cargo when the download fails. HERD_GAMEPAD_BUILD=source skips
+# Order: the GitHub release binary for this platform and version, verified by SHA-256
+# and its embedded source commit against a clean Git checkout; otherwise build with cargo. HERD_GAMEPAD_BUILD=source skips
 # the download (install.sh sets it so a development checkout builds local changes); any other
 # value is an error. HERD_GAMEPAD_RELEASE_URL overrides the release base URL; tests point it
 # at a directory.
 set -eu
-root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
+root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
 out=${HERD_GAMEPAD_OUT:-"$root/bin/herd-gamepad"}
 if [ -z "${HERD_GAMEPAD_OUT:-}" ] && [ -x "$out" ] && [ ! -f "$root/Cargo.toml" ]; then
   exit 0
@@ -18,12 +18,28 @@
 esac
 repo=PatrickPriestley/herdr-gamepad
 
-# The SHA-256 of a file, from whichever of shasum and sha256sum exists.
+# The SHA-256 of a file, from whichever tool exists. Capture the tool first so a
+# successful cut cannot hide a failing checksum command.
 sha256_of() {
-  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
-  elif command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
+  if command -v shasum >/dev/null 2>&1; then digest=$(shasum -a 256 "$1") || return 1
+  elif command -v sha256sum >/dev/null 2>&1; then digest=$(sha256sum "$1") || return 1
   else return 1
   fi
+  printf '%s\n' "$digest" | cut -d' ' -f1
+}
+
+# Git identity is useful only for this checkout, with no local source changes.
+# A source archive or a directory nested in another repository must build from source.
+checkout_revision() {
+  checkout_root=$(git -C "$root" rev-parse --show-toplevel 2>/dev/null) || return 1
+  [ "$checkout_root" = "$root" ] || return 1
+  checkout_head=$(git -C "$root" rev-parse --verify HEAD 2>/dev/null) || return 1
+  changes=$(git -C "$root" status --porcelain --untracked-files=normal 2>/dev/null) || return 1
+  if [ -n "$changes" ]; then
+    echo "herd-gamepad: checkout is not clean; building its local sources" >&2
+    return 1
+  fi
+  printf '%s\n' "$checkout_head"
 }
 
 # Download and verify the release binary into $1; non-zero with the reason on stderr otherwise.
@@ -51,8 +67,21 @@
     echo "herd-gamepad: checksum mismatch for $asset (want ${want:-nothing}, got ${got:-nothing}); not installing it" >&2
     rm -rf "$dl"; return 1
   fi
-  if ! tar -xzf "$dl/$asset" -C "$dl" herd-gamepad || ! cp "$dl/herd-gamepad" "$1"; then
+  if ! tar -xzf "$dl/$asset" -C "$dl" herd-gamepad herd-gamepad.commit; then
     echo "herd-gamepad: release asset $asset did not contain a usable herd-gamepad binary" >&2
+    rm -rf "$dl"; return 1
+  fi
+  # Downloads may take minutes; validate the checkout after downloading and extracting.
+  revision=$(checkout_revision) || {
+    echo "herd-gamepad: cannot verify a clean Git checkout; building from source" >&2
+    rm -rf "$dl"; return 1
+  }
+  if ! built_revision=$(cat "$dl/herd-gamepad.commit") || [ "$built_revision" != "$revision" ]; then
+    echo "herd-gamepad: release asset source commit mismatch; building this checkout" >&2
+    rm -rf "$dl"; return 1
+  fi
+  if ! cp "$dl/herd-gamepad" "$1"; then
+    echo "herd-gamepad: could not stage the release binary" >&2
     rm -rf "$dl"; return 1
   fi
   rm -rf "$dl"
--- a/.github/workflows/ci.yml
+++ b/.github/workflows/ci.yml
@@ -50,7 +50,8 @@
         run: |
           asset=herd-gamepad-${{ matrix.target }}.tar.gz
           mkdir -p dist
-          tar -czf "dist/$asset" -C target/release herd-gamepad
+          git rev-parse HEAD > target/release/herd-gamepad.commit
+          tar -czf "dist/$asset" -C target/release herd-gamepad herd-gamepad.commit
           (cd dist && shasum -a 256 "$asset" > "$asset.sha256")
           cat "dist/$asset.sha256"
       - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7
--- a/README.md
+++ b/README.md
@@ -14,15 +14,19 @@
 Requirements:
 
 - macOS and Herdr 0.9.1 or later.
-- Git. A Rust toolchain (`cargo`) and the Xcode Command Line Tools are needed only when no
-  prebuilt binary matches your Mac: the fallback build links with the system linker.
+- Git. A Rust toolchain (`cargo`) and the Xcode Command Line Tools are needed when no
+  verified prebuilt binary matches your Mac and source checkout.
 - `whisper-cli` from Homebrew's `whisper-cpp` package.
 - A controller paired with macOS and microphone permission for the terminal app.
 
-Herdr runs the build script during installation. It downloads the release binary for your
-Mac (Apple silicon or Intel), checks its SHA-256 against the checksum published with it, and
-only compiles from source with the committed Cargo.lock when no binary is available.
-Set `HERD_GAMEPAD_BUILD=source` to compile regardless.
+Herdr runs the build script during installation. New releases will include binaries for
+Apple Silicon and Intel Macs. The installer verifies the archive's SHA-256 and its embedded
+source commit against a clean Git checkout before using a binary. A different commit,
+local changes, missing Git metadata, or unavailable/unverifiable assets trigger a source
+build with the committed Cargo.lock. Set `HERD_GAMEPAD_BUILD=source` to compile regardless.
+
+Releases through v0.2.0 predate prebuilt assets and require Rust and the Xcode Command
+Line Tools.
 
 ```sh
 brew install whisper-cpp
@@ -150,8 +154,10 @@
 ## Publishing a release
 
 Releases publish automatically after a stable version tag passes both macOS CI jobs.
-Distribution remains source-built; the release contains GitHub's source archives and
-generated release notes, not prebuilt binaries.
+The tested macOS jobs package Apple Silicon and Intel binaries with their tested commit
+metadata and SHA-256 checksums. The publish job attaches both archives and checksums after
+creating the release with source archives and generated notes. Existing releases through
+v0.2.0 remain source-built; their tags and assets are not rewritten.
 
 First, open and merge a version-bump PR that updates all three versions to the same
 value: `Cargo.toml`, the `herd-gamepad` package entry in `Cargo.lock`, and
--- a/CHANGELOG.md
+++ b/CHANGELOG.md
@@ -1,4 +1,9 @@
 # Changelog
+
+## Unreleased
+
+- Install tested macOS release binaries only when checksums and embedded source commits match a clean checkout; otherwise build from source.
+- Publish Apple Silicon and Intel archives with commit metadata and SHA-256 checksums.
 
 ## 0.2.0
 
@@ -17,7 +22,6 @@
 - Keep the mapping overlay on demand; starting no longer opens a daemon tab.
 - Upgrade from v0.1.0 by closing its old Gamepad pane once before starting.
 - Publish stable version tags automatically after both macOS CI jobs pass, with version and tag validation.
-- Install the tested macOS release binary (checksum-verified) instead of compiling; cargo stays as the fallback and as the development path.
 
 ## 0.1.0
 
```

Run `sh tests/build.sh`, `python3 -m unittest discover -s tests -p 'test_release*.py'`, `cargo test --locked`, `HERD_GAMEPAD_BUILD=source sh scripts/build.sh`, and `actionlint .github/workflows/ci.yml`. Expected: 40 installer cases, 13 release tests, 168 Rust tests, a successful source build, and no workflow lint errors.

Compare applied files with the verified assembly below. If byte-identical, reuse its mutation evidence rather than rerunning unchanged code. If code changes, rerun the affected guard campaign. Commit: `fix: verify prebuilt binaries match the requested checkout`.

## Task 3: review and update PR #5

Use a fresh implementer, then sequential spec and quality reviews, then the different-model final review. Push the existing PR branch and update its description to describe commit-bound installation and the actual coverage. Wait for both macOS jobs. Download the new tested CI archive and checksum, check out its stamped commit, and exercise no-cargo installation from a file:// fixture without replacing the user's installed plugin. End-to-end GitHub release asset publication still belongs to the next release; do not claim this PR created those assets.

## Verification evidence

- Scratch: `/tmp/pr5-fix-scratch`; exact final assembly: `/tmp/pr5-fix-plan-assembled`.
- Patches: `/tmp/pr5-fix-tests.patch` and `/tmp/pr5-fix-code.patch`.
- `/tmp/pr5-assembled-red.log`: 22 positive installer assertions fail on the baseline.
- `/tmp/pr5-assembled-package-red.log`: permanent packaging test fails because the old workflow omits commit metadata.
- `/tmp/pr5-assembled-green.log`: all 40 installer cases pass.
- `/tmp/pr5-assembled-release-green.log`: all 13 release tests pass.
- `/tmp/pr5-rust-green.log` and `/tmp/pr5-assembled-rust-green.log`: all 168 Rust tests pass.
- `/tmp/pr5-source-build.log`: actual source-build entrypoint passes; binary reports 0.2.0.
- `/tmp/pr5-assembled-actionlint.log`: workflow lint passes.
- `/tmp/pr5-mutate-build.py`, `/tmp/pr5-build-mutations.log`, and `/tmp/pr5-build-mutations/results.json`: 19 installer guard/timing mutations caught, zero survivors. Includes both checksum tool branches, absent-tool failure, both checksum statuses, canonical root, each Git status, root match, dirty state, checkout validation, digest equality, extraction status/member, metadata status/equality, staging-copy status, and moving validation before the download.
- `/tmp/pr5-package-no-stamp-permanent-mutant.log` and `/tmp/pr5-package-no-member-permanent-mutant.log`: the permanent CI packaging test catches removal of the tested-HEAD stamp or archive member.
- The first broad mutation pass exposed checksum-helper status and symlink-invocation coverage gaps; those tests were strengthened and all final mutations were rerun successfully. No surviving guard is being waived.
