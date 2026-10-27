# Changelog

## Unreleased

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
