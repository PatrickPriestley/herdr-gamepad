# Changelog

## Unreleased

- Add optional dictation preview directly in the agent input field, with Cross to submit.
- Make saving preview Off restore automatic sending for subsequent recordings.

- Resolve worktrees from the focused pane when its space was opened outside a Git repository.

- Add the controller actions menu for spaces, worktrees, pane actions, settings, logs and controls.
- Keep menu input responsive during reads and mutations, preserve pending operations across popup sessions, and require live renderer acknowledgements for activation.
- Save label and voice preferences with conflict detection and format-preserving atomic writes.
- Keep ongoing dictation on its original settings when a settings save finishes after the menu closes.
- Match the Controls help text to the selected PlayStation, Xbox or combined button labels.
- Install tested macOS release binaries only when checksums and embedded source commits match a clean checkout; otherwise build from source.
- Publish Apple Silicon and Intel archives with commit metadata and SHA-256 checksums.

## 0.2.0

- Answer a waiting agent with one Cross press from Navigate or Respond.
- Interact with non-agent panes such as PR Tracker using arrows, Enter, Space, and Escape.
- Use Cross to enter a pane, Circle to return to navigation, and Share/Create for Escape.
- Keep D-pad pane navigation available even when an agent is waiting for input.
- Show familiar PlayStation button names with explicit Xbox equivalents in close controls.
- Keep close-menu feedback current and bound error notifications to avoid stale popup queues.
- Deliver completed dictation without waiting for another controller button press.
- Add controller close-pane and close-space actions with explicit target confirmation and cancellation.
- Submit dictation automatically to working agents as well as idle/done agents; no extra Enter press.
- Run Gamepad as a Herdr background action with explicit Start and Stop controls.
- Cancel active voice work and wait for shutdown on Stop.
- Add on-demand recent logs in an overlay, with bounded log rotation.
- Keep the mapping overlay on demand; starting no longer opens a daemon tab.
- Upgrade from v0.1.0 by closing its old Gamepad pane once before starting.
- Publish stable version tags automatically after both macOS CI jobs pass, with version and tag validation.

## 0.1.0

Initial macOS release:

- Navigate Herdr panes, tabs, and spaces with a game controller.
- Respond to agent prompts with deliberate confirmation and pane-bound targeting.
- Hold a stick button to dictate through local whisper.cpp transcription.
- Display neutral control names alongside Xbox and PlayStation equivalents.
- Follow UI focus from a background daemon; cancel recordings on disconnect.
- Bound subprocess execution and clean up timed-out process groups.

Distribution builds from source. DualSense has been exercised locally; other controllers
require hardware verification. Analog-stick movement, touchpad actions, and arbitrary
button remapping are not implemented.
