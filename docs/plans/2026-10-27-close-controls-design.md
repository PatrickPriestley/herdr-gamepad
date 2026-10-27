# Close pane and space controls

## Intent

Close the selected Herdr pane or space with a controller, without opening a permanent
control pane and without repurposing normal navigation or agent-response buttons.

## Probed facts

- The installed CLI exposes `herdr pane close <pane_id>` and
  `herdr workspace close <workspace_id> [--group]`.
- A disposable workspace and split were created with returned opaque IDs. Both close
  commands returned `type: ok` without prompting. An immediate read still reported the
  old pane count; later reads returned `pane_not_found` / `workspace_not_found`.
  Therefore acceptance is asynchronous and must not be described as completed closure.
- The workspace command will be invoked without `--group`; only the named space is targeted.
- Existing configuration permits voice on right stick, left stick, or Menu. A fixed Menu
  binding alone would silently collide with a supported configuration.

## Interaction

Menu / Options opens an on-demand notification containing the close actions. Left face
(X / Square) selects Close pane; top face (Y / Triangle) selects Close space. Selection
shows a second notification naming the target and explaining that running processes
will stop. Bottom face (A / Cross) confirms. Right face (B / Circle), Back / Select, or
the menu button cancels. No default close target is selected. With voice bound to Menu,
left stick click opens/cancels actions instead.

Opening from Respond returns to Navigate. While actions are open, agent keys,
navigation and recording starts are suppressed. Normal controls resume on cancellation
or one confirmed close. Dictation must finish before opening actions. No success toast
claims that asynchronous closure has completed; CLI errors use existing refusal feedback.

A direct close shortcut would be fewer presses but easier to trigger accidentally.
A dedicated overlay would persist visibly but needs a second UI process and input
coordination. Existing notifications keep this initial action menu small and on demand.

## State and safety

The pure core stores the original pane ID, workspace ID, and optional selected kind.
Both nonempty IDs are required. Each menu press checks the current focused pane and
space against that captured pair; mismatch or missing focus cancels. The emitted close
uses the captured explicit ID, never implicit current focus. Taking the state before
confirmation consumes the request once. Disconnect and failed snapshot acquisition
clear pending close intent. Space closure does not use group closure.

## Validation

Test the complete open/select/confirm flows for panes and spaces, every cancellation
button at both stages, no-selection confirmation, focus movement/missing context/empty
IDs, shell panes, Respond isolation, voice collisions and active voice refusal, disconnect,
snapshot failure and command failure feedback. Keep existing navigation/dictation tests.
Run cargo-mutants across the changed pure engine and mutation checks of adapter guards.
Hardware button behavior remains a hands-on check after installation; automated tests
cover the same logical button inputs without closing user terminals.
