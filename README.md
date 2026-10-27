# herd.gamepad

A Herdr plugin that lets an Xbox controller drive Herdr: the D-pad moves between panes,
the bumpers and triggers between spaces and tabs, Y jumps to the agent that needs you,
A and B answer its prompts, and holding the right stick click dictates a prompt through
whisper.cpp on your machine.

## Install

    brew install whisper-cpp        # whisper-cli
    ./install.sh                    # builds, copies to ~/.local/share/herdr-plugins/gamepad, links it

Pair the controller in System Settings > Bluetooth (hold the pairing button on the
controller until the Xbox button blinks fast). Then, in Herdr's command palette:
"Gamepad: set up voice" (downloads the 148 MB base.en model once), then "Gamepad: start".

The first time you hold the voice button, macOS asks whether the terminal app may use the
microphone; if that is denied, every hold ends with a "Recording failed" toast (allow it in
System Settings > Privacy & Security > Microphone). The daemon's log is in the "Gamepad"
tab that "Gamepad: start" opens.

## Buttons

See "Gamepad: show the mapping" in the palette, or `herd-gamepad mapping`.

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
