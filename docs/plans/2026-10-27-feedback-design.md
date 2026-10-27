# Current close controls and bounded feedback

Herdr0.9.1 source shows custom notifications displayed five seconds each, queue limit eight, and no replacement/dismiss API. Generic status toasts therefore accumulate stale instructions. Installed popup API returns only type:ok and preserves underlying focused pane; parent probed it successfully with mapping popup then terminated only that probe process. A new manifest popup entrypoint avoids CLI help lag. Per-menu unique directory, state revision, renderer acknowledgement and OS flock lease provide current display ownership without PID reuse or global popup close. A local subprocess probe confirmed lock held while child alive and released automatically on exit.

Only explicit controller Menu opens this popup. It polls/repaints the latest state, not a queue. Confirm requires an alive renderer and acknowledgement of exact current selection revision. Manual dismissal, startup failure/timeout, focus change, disconnect, snapshot failure and Stop cancel and clear only this menu. Closing is an explicit pinned pane/workspace action.

Routine feedback remains in logs. Sound::Request marks important errors, throttled globally to one external notification every five seconds with excess messages dropped but logged. Instructional Respond prompts change to Sound::None; close instructions are rendered only by popup. Other plugins may still delay rare error notifications; this plugin does not create a local queue. No automatic popup for errors.

## Verified actual popup

The parent ran `/tmp/probe-gamepad-popup.py` against a temporary plugin with its own ID `herd.gamepad-feedback-probe`, manifest `placement = "popup"`, and this renderer. CLI open without a placement override returned `type:ok`; renderer acknowledged revision 1, atomic state replacement was acknowledged as revision 2, and deleting the state exited the renderer and released its flock lease. The temporary plugin was unlinked. No installed gamepad plugin settings were changed. Probe evidence is at `/var/folders/fr/qg63wgbx36xck6qcw_61bq6m0000gn/T/gamepad-popup-live-05tqb2qw`.

Startup is allowed two seconds for renderer ownership after launch; a missing renderer then cancels and reports an error once. A renderer which previously held the lease and exits cancels silently. Confirm is allowed only for a current live renderer's exact revision. Same-process concurrent test directories exposed macOS timestamp resolution collisions, so unique paths combine process ID, timestamp and an atomic sequence; no PID signal is used for production ownership.

Herdr0.9.1 primary `src/app/popup.rs` dismiss path calls `shutdown_terminal_runtime` (locally captured at `/tmp/herdr-popup.rs`), so dismissal terminates the renderer and releases its lease. The acknowledgement proves stdout was flushed for that revision, not that a terminal compositor has painted every pixel; normal terminal-frame latency remains. No global popup-close command is used.
