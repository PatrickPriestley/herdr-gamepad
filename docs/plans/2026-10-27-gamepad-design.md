# Xbox controller for Herdr: design

Status: accepted 2026-10-27. Implementation plan: `2026-10-27-gamepad-implementation.md`.

## Goal

Drive Herdr from an Xbox controller: move focus between panes, tabs and spaces, answer an
agent's option and approval prompts, and dictate a prompt by holding a button.

## What Herdr offers (probed 2026-10-27, Herdr 0.9.1, protocol 22)

Herdr has no controller or voice input of its own: `herdr --default-config` lists keyboard
bindings only and `herdr api schema` has no such method. Everything needed is on the CLI:

| Need | CLI | Notes |
|---|---|---|
| focused pane and its agent | `herdr pane current` | `result.pane.{pane_id,workspace_id,tab_id,agent,agent_status}` |
| spaces in sidebar order | `herdr workspace list` | `result.workspaces[].{workspace_id,number,focused}` |
| tabs of a space in order | `herdr tab list --workspace W` | `result.tabs[].{tab_id,number,focused}` |
| agents and their state | `herdr agent list` | `result.agents[].{pane_id,workspace_id,agent_status}` |
| focus | `workspace focus W`, `tab focus T`, `pane focus --direction D --pane P`, `agent focus P` | |
| zoom | `pane zoom --pane P --toggle` | |
| keys to an agent | `agent send-keys P up` | validates the agent is there (`agent_not_found`) and the key (`invalid_key`) before writing |
| text | `agent prompt P TEXT` (submits), `pane send-text P TEXT` (no Enter) | |
| toast | `notification show TITLE --body B --sound none\|done\|request` | |

CLI round trip measured at 6.5 ms median; the raw socket at 0.2 ms. The CLI is the
versioned surface the pr-tracker plugin already uses, so the bridge shells out to it.

## Shape

A Rust Herdr plugin, `herd.gamepad`, one binary `herd-gamepad`, laid out like
`~/Code/herdr-pr-tracker`: `herdr-plugin.toml`, `scripts/build.sh`, `install.sh`.

- `herd-gamepad serve` is the daemon. It runs in a plugin pane with placement `tab`, opened
  by the "Gamepad: start" action (`ensure`), so its log is a tab away and never in the layout.
- `gilrs` 0.11 reads the controller through IOKit, no SDL, no driver (probed: builds and
  initialises on macOS 15.6; Xbox controllers pair over Bluetooth natively).
- The core is a pure state machine: `(Input, &Snapshot) -> Vec<Action>`. Adapters around it
  are thin: the controller reader, the `herdr` runner, the recorder, the transcriber.

## Buttons

Two modes. The View button (left of the Xbox button) returns to Navigate.

Navigate (default):

| Button | Action |
|---|---|
| D-pad | focus the pane in that direction |
| LB / RB | previous / next space, wrapping, by sidebar number |
| LT / RT | previous / next tab of the current space, wrapping |
| Y | focus the next agent that needs you: the first `blocked` after the focused pane, in sidebar order (space number, then Herdr's listing order), else the first `done`; a toast when there is none |
| X | toggle zoom on the focused pane |
| A | enter Respond mode (a toast if the pane holds no agent) |
| B, Menu | nothing in v1 |

Respond:

| Button | Action |
|---|---|
| D-pad | `agent send-keys` up / down / left / right |
| A | enter |
| B | esc |
| View | back to Navigate |
| LB, RB, LT, RT, Y, X | as in Navigate, and back to Navigate |

Both modes: hold the right stick click to record; release to transcribe.

Auto-switch: a D-pad press while the focused agent is `blocked` switches to Respond first
and is then handled there (a toast says so). A does not auto-switch and never sends Enter on
the press that entered Respond, so an approval is always two deliberate presses.

Respond mode is tied to the pane it was entered on; a press with another pane focused
leaves it and sends nothing. Every Enter needs its own acknowledging press (D-pad, B, or a
swallowed A) while the agent is blocked, so no dialog is ever confirmed by a single press.
Disconnecting leaves Respond and discards a recording in progress. A transcript goes only
to the pane focused when the voice button was released.

Keys go through `agent send-keys` with the pane id as target, never `pane send-keys`, so
Herdr refuses them when no agent occupies the pane. The daemon never answers a dialog on
its own.

## Voice

Hold right stick click: `cpal` 0.18 opens the default input at 48 kHz mono f32 (16 kHz is not
offered by the MacBook microphone; probed). Release: the samples go to a 16-bit WAV in the
plugin state directory and `whisper-cli` (Homebrew `whisper-cpp` 1.9.5) transcribes it:

    whisper-cli -m <model> -f <wav> -nt -np -l en

Probed: whisper-cli resamples 48 kHz itself; stdout carries only the transcript (one leading
newline); silence yields `[BLANK_AUDIO]`; a missing file exits 2; `ggml-base.en.bin` takes
0.4 to 0.6 s warm for a sentence on this machine and 18 s cold the first time.

Routing by the focused agent's state:

| state | what happens |
|---|---|
| idle, done | `agent prompt` submits it; toast "Sent: …" |
| working | `pane send-text` types it without Enter; toast "Typed, not sent: …" |
| blocked, unknown, no agent | dropped; toast says why |
| focus moved since you spoke, or nothing focused when you spoke | dropped; toast says which |
| blank transcript | dropped; toast "Heard nothing" |

Recordings shorter than 0.3 s are dropped; longer than 60 s are cut. `herd-gamepad setup`
downloads the model to `<state>/models/ggml-base.en.bin` when missing.

## Config

`<plugin config dir>/config.toml` (`herdr plugin config-dir herd.gamepad`):

    [whisper]
    model = ""          # empty: <state>/models/ggml-base.en.bin
    language = "en"
    [voice]
    button = "rstick"   # or "lstick", "menu"
    max_seconds = 60

Button remapping beyond the voice button is deferred.

## Feedback and failure

Every mode change, rejected action and failed `herdr` call produces a short toast and a
stderr log line in the daemon pane. Controller disconnect and reconnect are `gilrs` events
and are toasted; the loop keeps running. A `herdr` error never stops the daemon.

## Testing

- `core` is tested without hardware: each button in each mode against snapshots built in the
  test, asserting the exact actions. Every guard has a test that fails when it is removed
  (`cargo mutants` runs on `src/core.rs`).
- `herdr` parsing is tested against JSON captured from the probes above, with a fake
  `Runner` that records argv.
- `voice::clean` is tested on real whisper output, including `[BLANK_AUDIO]`.
- The manifest test checks every entry calls a real subcommand.
- End to end needs a paired controller: see the plan's last task.

## Deferred

Analog sticks, pane resize and move, workspace picker, dictation fallback, the raw socket
protocol, Linux, a launchd service, full button remapping.


## Background lifecycle update (2026-10-27)

The background-operation plan supersedes the original daemon-pane design. Start runs
`serve` as a Herdr-owned action with no terminal pane. Stop requests cancellation for
the current instance and waits for recording/transcription cleanup and lock release.
Logs rotate in plugin state (1 MiB plus one previous file). Show logs and Show mapping
open explicit overlays; neither is opened by Start. See
[background-operation plan](2026-10-27-background-operation.md).
