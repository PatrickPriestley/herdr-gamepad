# herd.gamepad

Drive [Herdr](https://herdr.dev) from a game controller. Move between panes, tabs and
spaces, answer an agent's prompts, drive ordinary panes such as the PR tracker, close what
you are done with, and dictate a prompt by holding a button. Speech is transcribed on your
Mac with whisper.cpp; nothing leaves it.

![herd-gamepad controls on a PS5 DualSense](docs/controller-dualsense.svg)

Actions follow button positions, not labels. An Xbox controller's A, B, X and Y sit where
Cross, Circle, Square and Triangle do and behave the same. The close popup names
PlayStation buttons with their Xbox equivalents; logs use physical names such as "bottom
face button". A DualSense has been exercised by hand; other models should work through the
same macOS input path but have not been checked on hardware.

## Install

You need macOS with Herdr 0.9.1 or later, Git, a current stable Rust toolchain (`cargo`)
with the Xcode Command Line Tools, `whisper-cli` from Homebrew's `whisper-cpp` package, a
controller paired with macOS, and microphone permission for your terminal app.

```sh
brew install whisper-cpp
herdr plugin install PatrickPriestley/herdr-gamepad --ref v0.2.0
```

This release builds from source: Herdr runs the plugin's build script during installation,
using the committed `Cargo.lock`. Then:

1. Pair the controller in System Settings > Bluetooth. On a DualSense, hold Create and the
   PS button until the light bar flashes. On an Xbox controller, hold its pairing button.
2. In Herdr's command palette, run "Gamepad: set up voice". It downloads the 148 MB
   `base.en` speech model once.
3. Run "Gamepad: start".

The first time you hold the voice button, macOS asks whether the terminal app may use the
microphone. If you decline, every hold ends with a "Recording failed" notification; allow
it under System Settings > Privacy & Security > Microphone.

If you installed an earlier copy with `./install.sh`, stop its daemon and run
`herdr plugin unlink herd.gamepad` before installing from GitHub. Upgrading from v0.1.0,
which ran in a pane, close that pane once before starting. Settings and the downloaded
model live in Herdr's per-plugin folders and survive reinstalls.

## Using it

The controller is always in one of three modes. **Navigate** is the default and moves you
around Herdr. **Respond** sends your presses to the agent in the focused pane. **Interact**
sends them to any other pane, such as a shell or the PR tracker.

**Navigate.** The D-pad focuses the neighbouring pane; it always does, even when an agent
is waiting. L1 and R1 step through spaces, L2 and R2 through tabs. Triangle jumps to the
next agent that is waiting for you, or the next one that has finished. Square zooms the
focused pane. Cross enters Respond on an agent pane and Interact on any other pane.

**Respond.** The D-pad moves through the agent's options, Cross sends Enter, Create sends
Escape, and Circle returns to Navigate without sending anything. If the agent is already
waiting at a question when you press Cross from Navigate, that single press enters Respond
and answers it. Respond is tied to the pane you entered it on: if focus moves elsewhere,
the next press returns to Navigate and sends nothing.

**Interact.** The first Cross only enters the mode. Then the D-pad sends arrow keys, Cross
sends Enter, Square sends Space, Create sends Escape, and Circle returns to Navigate. The
app decides what each key means: in the PR tracker, left and right choose a category, up
and down move through items, Square opens details. If focus moves or an agent appears in
the pane, the next press returns to Navigate and sends nothing. Dictation still needs an
agent pane.

In both Respond and Interact, the shoulders, triggers and Triangle keep their Navigate
actions and return to Navigate. Square also does, except in Interact where it is Space.

**Dictate.** Hold the right stick click, speak, release. The text goes to the pane that was
focused when you released: an idle, done or working agent receives it as a submitted
prompt. It is dropped, with a notification saying why, if focus moved since you spoke, the
pane has no agent, or the agent is waiting at a dialog. Recording stops if the controller
disconnects, and the audio is discarded.

**Close a pane or space.** Press Options to open the close menu, a small popup. Square
selects "close pane", Triangle selects "close space". Wait until the popup names the
target, then press Cross to confirm. Circle, Create or Options again cancels. Closing stops
whatever runs there; closing a space closes all its tabs and panes, but not its worktree
group. A focus change, a disconnect or dismissing the popup cancels the pending close.
Finish dictating before opening the menu. If your voice button is Options, the left stick
click opens the menu instead.

**Feedback.** Mode changes and successful sends are written to the Gamepad logs rather
than shown as notifications. Errors are shown as notifications at most once every five
seconds and are always logged. "Gamepad: show logs" opens a snapshot of recent lines;
reopen it to refresh. Logs rotate at 1 MiB with one previous file kept, and can contain
transcript previews.

**Start and stop.** "Gamepad: start" listens in the background; nothing opens on screen.
"Gamepad: stop" ends listening and cancels any recording or transcription in flight.
Herdr owns the background task, so start it again after restarting Herdr. "Gamepad: show
the mapping" opens the reference card, which also shows your configured voice button.

To see what a controller sends, run `herd-gamepad probe` from a terminal; Ctrl-C stops it.

## Buttons

| Position | Xbox | PlayStation | Navigate | Respond | Interact | Close menu |
|---|---|---|---|---|---|---|
| Bottom face | A | Cross | Enter Respond or Interact; answers a waiting agent | Enter | Enter | Confirm |
| Right face | B | Circle | | Back to Navigate | Back to Navigate | Cancel |
| Left face | X | Square | Toggle zoom | Toggle zoom, back to Navigate | Space | Select "close pane" |
| Top face | Y | Triangle | Next agent needing you | Same, back to Navigate | Same, back to Navigate | Select "close space" |
| D-pad | | | Focus neighbouring pane | Arrow keys | Arrow keys | |
| Shoulders | LB / RB | L1 / R1 | Previous / next space | Same, back to Navigate | Same, back to Navigate | |
| Triggers | LT / RT | L2 / R2 | Previous / next tab | Same, back to Navigate | Same, back to Navigate | |
| Back / Select | View | Create | | Escape | Escape | Cancel |
| Menu | Menu | Options | Open close menu | Open close menu | Open close menu | Cancel |
| Right stick click | | R3 | Hold to dictate | Hold to dictate | Hold to dictate | |
| Left stick click | | L3 | Opens the close menu when voice is on Options | | | |

Unused: touchpad, PS button, mic button, analog stick movement.

## Settings

`$(herdr plugin config-dir herd.gamepad)/config.toml`:

```toml
[whisper]
model = ""          # empty: <state dir>/models/ggml-base.en.bin
language = "en"

[voice]
button = "rstick"   # rstick, lstick or menu
max_seconds = 60    # recordings are cut here; 1 to 3600
```

A value of the wrong type is an error, not a silent default.

## Updating and removing

Run "Gamepad: stop", then install the new release tag:

```sh
herdr plugin install PatrickPriestley/herdr-gamepad --ref <release-tag>
```

and run "Gamepad: start". A running daemon keeps its old executable until restarted.

To remove the plugin, stop its daemon and run `herdr plugin uninstall herd.gamepad`.

## Development

```sh
git clone https://github.com/PatrickPriestley/herdr-gamepad.git
cd herdr-gamepad
cargo test --locked
sh tests/build.sh
./install.sh
```

`install.sh` builds and links the local copy into Herdr. CI tests and builds on Apple
silicon and Intel macOS runners and checks the installed layout; controller and microphone
behaviour still need a hand on real hardware.

Design notes and implementation plans live under `docs/plans/`. Start with
`docs/plans/2026-10-27-gamepad-design.md`.

## Publishing a release

A release is a stable `vMAJOR.MINOR.PATCH` tag on `main`. Before tagging, merge a PR that
sets the same version in `Cargo.toml`, the `herd-gamepad` entry in `Cargo.lock` and
`herdr-plugin.toml`, and updates `CHANGELOG.md`. Then:

```sh
git switch main
git pull --ff-only
git tag -a v0.2.1 -m "Release v0.2.1"
git push origin v0.2.1
```

CI checks that the tag matches all three version files, that the commit is on `main` and
that the remote tag still points at the tested commit. After both macOS jobs pass, it
creates the release with generated notes. Releases are source archives; there are no
prebuilt binaries yet. Prerelease tags are not supported.

If CI or the GitHub API fails transiently, rerun the tag's workflow; rerunning an already
published release leaves it unchanged. An existing draft release is left for you to review.
For a version or content mistake, fix it in a PR and cut a new tag; never move a published
tag.

## License

MIT; see [LICENSE](LICENSE). Herdr, whisper.cpp and the downloaded model have their own
licenses and distribution terms.
