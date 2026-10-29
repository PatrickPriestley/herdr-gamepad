# Joystick scrolling

## Goal
Use vertical right-stick movement to scroll the selected pane in Navigate, Respond and Interact, including expanded panes. Keep voice recording on the stick click.

## Probed facts
- gilrs normalizes stick Y so positive is up, including its macOS Y reversal.
- Herdr pane.scroll changes terminal history only. Wheel input instead chooses application mouse reporting, alternate scrolling or terminal history.
- Herdr injects HERDR_SOCKET_PATH into plugin commands. Its JSON API is newline-delimited request/response.
- A companion local Herdr change adds pane.scroll_wheel with pane_id, direction (up/down), lines (1–10), and focused_only. Stable Herdr currently lacks this method.

## Decisions
- Use the wheel-routing API, never substitute PageUp, arrow keys or guessed terminal escapes. That preserves application-owned scrolling semantics.
- Begin at 35% vertical deflection, stop within 20%, repeat at most every 100 ms, and use one to three wheel steps according to deflection. Horizontal and left-stick motion remain unused.
- Keep scrolling off the controller/voice loop. A single worker observes the latest intent; there is no command backlog. Expired intents stop repeating if the daemon loop stalls.
- Bind each gesture to the selected pane. Server-side focused_only validation rejects a focus race. A button press, menu, dictation, disconnect or error stops the gesture; a tilted stick must return to neutral before rearming.
- Missing API support disables only that gesture and produces one diagnostic. Existing controls keep working. Do not claim a stable Herdr version supports this until the companion change is available.

## Work and validation
1. Add right-stick axis adaptation in pad.rs and tests for axis selection, normalized sign, finite values and bounds.
2. Add scrolling.rs pure gesture gate and coalescing worker with tests for dead zone, hysteresis, pacing, target ownership, suppression, expiration and failures.
3. Add bounded scroll_rpc.rs socket transport; fake sockets cover framing, errors, response limits and deadlines.
4. Connect serve.rs before menu/core dispatch, with integration tests that show scrolling does not request CLI snapshots or become application keys.
5. Update mapping/help and README with controls and the patched-Herdr requirement.
6. Observe assertion failures before implementing each behavior, mutation-check guards, run the complete Rust suite, and obtain independent review before pushing. Hardware validation remains manual; do not send test input to live user panes.
