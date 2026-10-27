# herd.gamepad

A Herdr plugin that lets a game controller drive Herdr: navigate panes, tabs and spaces,
answer agent prompts, and dictate through whisper.cpp on your machine.

Controls follow physical button positions. The mapping card shows Xbox and PlayStation
names side by side; prompts and logs use neutral names such as "bottom face button".
Controller compatibility still depends on macOS and the input backend. A DualSense has
been exercised locally; other controller models need their own hardware checks.

## Install

    brew install whisper-cpp        # whisper-cli
    ./install.sh                    # builds, copies to ~/.local/share/herdr-plugins/gamepad, links it

Pair your controller in System Settings > Bluetooth. For Xbox, hold its pairing button;
for PS5 DualSense, hold Create + PS until the light bar flashes. Then, in Herdr's command
palette: "Gamepad: set up voice" (downloads the 148 MB base.en model once), then
"Gamepad: start".

The first time you hold the voice button, macOS asks whether the terminal app may use the
microphone; if that is denied, every hold ends with a "Recording failed" toast (allow it in
System Settings > Privacy & Security > Microphone). The daemon's log is in the "Gamepad"
tab that "Gamepad: start" opens.

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

D-pad moves between panes in Navigate, and between options in Respond. On a blocked
agent, D-pad automatically enters Respond. Each confirmation requires acknowledgement
first; follow the on-screen prompt. Face-button positions always determine actions,
even on controllers with different lettering.

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
