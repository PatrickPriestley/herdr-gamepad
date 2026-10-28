# herdr.gamepad

Drive [Herdr](https://herdr.dev) from a game controller: move between panes, tabs and
spaces, answer agents, drive ordinary panes such as the PR tracker, close panes, and
dictate prompts. Speech is transcribed locally with whisper.cpp.

![herd-gamepad controls on a PS5 DualSense](docs/controller-dualsense.svg)

Actions follow button positions: Xbox A, B, X and Y behave like Cross, Circle, Square and
Triangle. Tested by hand on a DualSense; other controllers are untested.

## Install

Requires macOS, Herdr 0.9.1+, a Rust toolchain with the Xcode Command Line Tools, and
Homebrew's `whisper-cpp`.

```sh
brew install whisper-cpp
herdr plugin install PatrickPriestley/herdr-gamepad --ref v0.2.0
```

1. Pair the controller in System Settings > Bluetooth (DualSense: hold Create + PS).
2. Run "Gamepad: set up voice" from the command palette. It downloads the speech model
   (see [Speech model](#speech-model)).
3. Run "Gamepad: start". Allow microphone access for your terminal app when macOS asks.

## Modes

- **Navigate** (default): D-pad moves between panes, L1/R1 between spaces, L2/R2 between
  tabs. Triangle jumps to the next agent that needs you; Square zooms the pane. Cross enters
  Respond on an agent pane or Interact on any other pane.
- **Respond**: keys go to the agent. D-pad moves through options, Cross is Enter, Create is
  Escape, Circle returns to Navigate. Pressing Cross on an agent that is already waiting
  answers it in one press.
- **Interact**: keys go to a non-agent pane. D-pad sends arrows, Cross Enter, Square Space,
  Create Escape, Circle returns to Navigate. In the PR tracker, left/right pick a category
  and Square opens details.

In Respond and Interact, the shoulders, triggers and Triangle act as in Navigate and return
there. Moving focus to another pane also returns to Navigate without sending a key.

**Dictate**: hold R3, speak, release. The text is submitted to the agent you were focused
on; it is dropped if focus moved, the pane has no agent, or the agent is at a dialog.

**Close**: Options opens a popup. Square picks "close pane", Triangle "close space"; once the
popup names the target, Cross confirms. Circle, Create or Options cancels.

Routine status goes to the log ("Gamepad: show logs"); only errors appear as notifications.
"Gamepad: show the mapping" shows the full card. "Gamepad: stop" stops listening.

## Buttons

| Position | Xbox | PlayStation | Navigate | Respond | Interact | Close menu |
|---|---|---|---|---|---|---|
| Bottom face | A | Cross | Respond / Interact | Enter | Enter | Confirm |
| Right face | B | Circle | | To Navigate | To Navigate | Cancel |
| Left face | X | Square | Zoom | Zoom, to Navigate | Space | Close pane |
| Top face | Y | Triangle | Next agent | Same, to Navigate | Same, to Navigate | Close space |
| D-pad | | | Focus pane | Arrows | Arrows | |
| Shoulders | LB / RB | L1 / R1 | Prev / next space | Same, to Navigate | Same, to Navigate | |
| Triggers | LT / RT | L2 / R2 | Prev / next tab | Same, to Navigate | Same, to Navigate | |
| Back | View | Create | | Escape | Escape | Cancel |
| Menu | Menu | Options | Close menu | Close menu | Close menu | Cancel |
| Right stick | | R3 | Hold to dictate | Hold to dictate | Hold to dictate | |

If the voice button is set to Options, L3 opens the close menu.

## Settings

`$(herdr plugin config-dir herd.gamepad)/config.toml`:

```toml
[whisper]
model = ""          # empty: the downloaded base.en model
language = "en"

[voice]
button = "rstick"   # rstick, lstick or menu
max_seconds = 60
```

### Speech model

Setup downloads `ggml-base.en.bin` (148 MB, English) from the
[whisper.cpp model repository](https://huggingface.co/ggerganov/whisper.cpp) into the
plugin's state folder, unless a model is already there. Start refuses to run without one.

To use another model, download it and set `model` to its path. Larger models (`small`,
`medium`) are more accurate and slower; for other languages use a model without `.en` and
set `language`.

## Updating and removing

Run "Gamepad: stop", reinstall with the new `--ref`, then "Gamepad: start".
To remove: stop, then `herdr plugin uninstall herd.gamepad`.

## Development

```sh
cargo test --locked
./install.sh        # build and link a local copy
```

Design notes are in `docs/plans/`. CI builds and tests on Apple silicon and Intel;
controller and microphone behaviour need real hardware.

## Releasing

Merge a PR that bumps the version in `Cargo.toml`, `Cargo.lock` and `herdr-plugin.toml` and
updates `CHANGELOG.md`, then tag `main`:

```sh
git switch main && git pull --ff-only
git tag -a v0.2.1 -m "Release v0.2.1" && git push origin v0.2.1
```

CI checks the tag matches the versions and publishes the release once both macOS jobs
pass. Never move a published tag; fix mistakes with a new one.

## License

MIT; see [LICENSE](LICENSE).
