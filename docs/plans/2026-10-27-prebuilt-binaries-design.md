# Prebuilt release binaries: design

Status: accepted 2026-10-27. Plan: `2026-10-27-prebuilt-binaries-implementation.md`.

## Goal

`herdr plugin install PatrickPriestley/herdr-gamepad --ref vX.Y.Z` installs in seconds on a
Mac without a Rust toolchain. Source builds stay as the fallback and as the development path.

## Probed facts (2026-10-27)

- `herdr plugin install owner/repo --ref TAG` clones the tag and runs the manifest's
  `[[build]]` command, `scripts/build.sh`, on the user's machine. A fresh clone of v0.1.0
  built in 17 s here with cargo present.
- The repo is public. An anonymous `curl -fsSL` of a release asset works for a public repo;
  for the private pr-tracker repo the same URL returns 404, so that plugin's download path
  only works for its owner.
- A file downloaded with curl carries no `com.apple.quarantine` attribute (checked with
  `xattr`), so Gatekeeper does not block an unsigned binary installed this way. Microphone
  permission attaches to the terminal app, not the binary; IOKit controller access needs no
  entitlement.
- pr-tracker's asset layout: `pr-tracker-<target>.tar.gz` holding the single binary, plus
  `<asset>.sha256` in `shasum -a 256` format (`<hex>  <name>`). Its checksum check matched.
- `uname -s; uname -m` on this machine: `Darwin arm64`. Runner targets: `macos-15` is
  `aarch64-apple-darwin`, `macos-15-intel` is `x86_64-apple-darwin`. `shasum` exists on
  macOS; `sha256sum` may not.
- Main's `publish` job runs `scripts/publish_release.py` on Ubuntu after both macOS jobs
  pass. That script has a mutation harness whose test asserts exactly 33 mutations, so it
  is not modified here. `gh release upload --clobber` replaces assets of the same name.
- Pinned action SHAs: `actions/upload-artifact@v7` = `cf430e030ddbb5b0abf93d22962f4752f3646cd9`,
  `actions/download-artifact@v7` = `37930b1c2abaa49bbe596cd826c3c89aef350131`.
- In-flight branches `feat/close-controls` and `fix/auto-submit-dictation` touch
  `README.md` and `CHANGELOG.md` but not `scripts/build.sh`, `tests/build.sh`, `install.sh`
  or `.github/workflows/ci.yml`.

## Decisions

1. **Prebuilt first, source fallback.** `scripts/build.sh` downloads
   `herd-gamepad-<target>.tar.gz` and its `.sha256` for the manifest's version from the
   GitHub release, verifies the digest, and installs the binary. When the download or the
   check fails it builds with cargo if cargo is present, otherwise it exits 1 with both
   remedies in the message. `HERD_GAMEPAD_BUILD=source` skips the download; `install.sh`
   sets it so a development checkout always builds local changes.
   Rejected: source first (pr-tracker's order) makes every user with Rust compile for a minute
   and gains nothing; the asset is the tested artefact.
2. **Fail closed on checksums.** An empty expected digest never matches. A mismatch leaves
   the installed binary untouched and falls back to cargo like any other download failure.
3. **Atomic replacement stays.** The downloaded or built binary is staged in a temp file
   next to `bin/herd-gamepad` and moved into place, preserving the existing inode test.
4. **Assets come from the tested macOS jobs.** Each matrix job packages its release binary
   and checksum into `dist/` and uploads them as an artifact. The `publish` job downloads
   both artifacts, runs the unchanged `publish_release.py` (which creates the release or
   reports it already exists), then uploads the assets with `gh release upload --clobber`.
   Rejected: building in the publish job (Ubuntu cannot build macOS binaries) and editing
   the Python publisher (its mutation count is pinned by a test).
5. **Installed-copy guard unchanged.** A copy with a binary and no `Cargo.toml` still
   does nothing.

## Testing

`tests/build.sh` grows from one case to five, each in a fixture with stubbed `curl` and
`cargo` on `PATH`: the existing inode case; a stub release served from a directory, no
cargo, installs the binary; a checksum mismatch leaves the old binary and uses cargo; a
404 with cargo present builds from source; `HERD_GAMEPAD_BUILD=source` never calls curl.
CI already runs `tests/build.sh` on both runners. The workflow is checked with a YAML parse
and, where available, `actionlint`; the upload step is exercised by the next tag.

## Trust boundary

The checksum is produced by the job that built the binary, so it proves integrity, not
authorship: the same level as pr-tracker. Signing and notarization are out of scope.

## Out of scope

Linux targets, notarization, changes to `publish_release.py`, retroactive assets for v0.1.0
(its users fall back to the source build).
