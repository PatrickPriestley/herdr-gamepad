# herd.gamepad

A Herdr plugin that lets a game controller drive Herdr: navigate panes, tabs and spaces,
answer agent prompts, and dictate through whisper.cpp on your machine.

Controls follow physical button positions. The mapping card shows Xbox and PlayStation
names side by side. The close popup names PlayStation buttons with explicit Xbox equivalents;
diagnostic logs retain physical names such as "bottom face button".
Controller compatibility still depends on macOS and the input backend. A DualSense has
been exercised locally; other controller models need their own hardware checks.

## Install

Requirements:

- macOS and Herdr 0.9.1 or later.
- Git, a current stable Rust toolchain (`cargo`), and Xcode Command Line Tools.
- `whisper-cli` from Homebrew's `whisper-cpp` package.
- A controller paired with macOS and microphone permission for the terminal app.

This release builds from source; it does not include prebuilt binaries. Herdr runs the
build script during installation, using the committed Cargo.lock.

```sh
brew install whisper-cpp
herdr plugin install PatrickPriestley/herdr-gamepad --ref v0.1.0
```

If you previously used `./install.sh`, stop its Gamepad daemon with Ctrl-C in the
Gamepad tab, then run `herdr plugin unlink herd.gamepad` before installing from GitHub.
Settings and the downloaded voice model stay in Herdr's per-plugin config/state folders.

Pair your controller in System Settings > Bluetooth. For Xbox, hold its pairing button;
for PS5 DualSense, hold Create + PS until the light bar flashes. Then, in Herdr's command
palette: "Gamepad: set up voice" (downloads the 148 MB base.en model once), then
"Gamepad: start".

The first time you hold the voice button, macOS asks whether the terminal app may use the
microphone; if that is denied, every hold ends with a "Recording failed" toast (allow it in
System Settings > Privacy & Security > Microphone).

## Background operation (next release)

"Gamepad: start" listens in the background without opening a pane. "Gamepad: stop"
ends listening and cancels active recording or transcription. Repeated Start/Stop is safe.
Herdr owns the background action; start it again after restarting Herdr.

"Gamepad: show logs" opens an overlay containing a snapshot of recent diagnostics;
reopen it to refresh. Logs rotate at 1 MiB and retain one previous file in the plugin
state directory (`daemon.log` and `daemon.log.1`). Logs can include transcript previews.
"Gamepad: show the mapping" opens the mapping overlay only when requested. Closing
either overlay leaves the background daemon running.

These controls are on the development branch and are not in the v0.1.0 release above.
When upgrading from v0.1.0, close its old Gamepad daemon pane once before starting the
new background version. Settings and downloaded models are preserved.

## Buttons

Open "Gamepad: show the mapping" in the palette, or run `herd-gamepad mapping`.
The card also shows your configured voice button.

| Physical control | Xbox | PlayStation | Action |
|---|---|---|---|
| Bottom face | A | Cross | Enter Respond; confirm in Respond |
| Right face | B | Circle | Escape in Respond |
| Left face | X | Square | Toggle zoom |
| Top face | Y | Triangle | Next agent needing attention |
| Left / right shoulder | LB / RB | L1 / R1 | Previous / next space |
| Left / right trigger | LT / RT | L2 / R2 | Previous / next tab |
| Back / Select | View | Create (PS5) / Share (PS4) | Return to Navigate |
| Right stick click | Right stick click | R3 | Hold to dictate (default) |
| Menu / Options | Menu | Options | Open close actions |

D-pad moves between panes in Navigate, and between options in Respond. On a blocked
agent, D-pad automatically enters Respond. Each confirmation requires acknowledgement
first; press Cross (Xbox: A) again to confirm. Face-button positions always determine actions,
even on controllers with different lettering.

Release the voice button to transcribe and automatically submit to an idle, done, or
working agent—no extra Enter or controller press is needed. If focus changed, the pane
has no agent, or the agent is blocked at a dialog or in an unknown state, the transcript
is not sent.

To close a pane or space, press **Menu / Options** to open the live close-actions popup. Press the
**Square (Xbox: X)** to select Close pane, or **Triangle (Xbox: Y)** to select Close space.
Wait for the selected target to appear in the popup, then press **Cross (Xbox: A)** to confirm.
**Circle (Xbox: B)**,
**Back / Select**, or the menu button cancels. Closing stops running processes in that
pane or space; closing a space includes all its panes and tabs, not its workspace group.
Closing the popup, a focus change, or a controller disconnect cancels the pending action. Finish dictation
before opening close actions. If `[voice] button = "menu"`, use **left stick click
(L3)** to open/cancel close actions instead.

Routine mode and successful-send feedback goes to Gamepad logs rather than notification popups.
Errors notify at most once every five seconds; every error is still logged. Herdr may delay an
error behind notifications from other plugins; close confirmation uses its own current display.

To inspect a controller's events, run `herd-gamepad probe`; Ctrl-C stops it.

## Settings

`$(herdr plugin config-dir herd.gamepad)/config.toml`:

    [whisper]
    model = ""          # empty: <state>/models/ggml-base.en.bin
    language = "en"
    [voice]
    button = "rstick"   # rstick, lstick or menu
    max_seconds = 60

## Design

`docs/plans/2026-10-27-gamepad-design.md`.

## Updates and removal

To install a newer release, use "Gamepad: stop" (v0.1.0: Ctrl-C in its Gamepad tab), then rerun
`herdr plugin install PatrickPriestley/herdr-gamepad --ref <release-tag>` and invoke
"Gamepad: start". A running daemon keeps its old executable until restarted.

To remove the plugin, stop its daemon, then run `herdr plugin uninstall herd.gamepad`.

## Development

```sh
git clone https://github.com/PatrickPriestley/herdr-gamepad.git
cd herdr-gamepad
cargo test --locked
./install.sh
```

The development installer builds and links a local copy. CI tests and builds on Apple
Silicon and Intel macOS; controller and microphone behaviour still needs hardware testing.

## Publishing a release

Releases publish automatically after a stable version tag passes both macOS CI jobs.
Distribution remains source-built; the release contains GitHub's source archives and
generated release notes, not prebuilt binaries.

First, open and merge a version-bump PR that updates all three versions to the same
value: `Cargo.toml`, the `herd-gamepad` package entry in `Cargo.lock`, and
`herdr-plugin.toml`. Update `CHANGELOG.md` with that release's changes too. Then tag the
merged commit from an up-to-date `main`, using the matching version (for example):

```sh
git switch main
git pull --ff-only
git tag -a v0.1.1 -m "Release v0.1.1"
git push origin v0.1.1
```

The tag must use stable `vMAJOR.MINOR.PATCH` format; prerelease tags are not supported.
CI checks that the tag matches all three version files, the commit is on `main`, and
the remote tag still points to the tested commit. After both macOS jobs pass, it
publishes the release automatically. No manual `gh release create` command is needed.

For a transient CI or GitHub API failure, rerun the failed tag workflow in GitHub Actions.
Rerunning an already published release leaves it unchanged. An existing draft must be
reviewed and handled manually. For version or content errors, fix them through a PR and
use a new correct version tag; do not move a published tag.

## License

MIT; see [LICENSE](LICENSE). Herdr, whisper.cpp, and the downloaded model have their own
licenses and distribution terms.
