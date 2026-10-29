# Joystick terminal scrollback

## Goal
Use vertical right-stick movement to scroll the selected pane's terminal history in Navigate, Respond and Interact, including expanded panes. Keep dictation on the stick click. Use released Herdr APIs without a custom Herdr build.

## Probed facts
- gilrs normalizes stick Y so positive is up, including its macOS Y reversal.
- Installed Herdr 0.9.1 exposes `pane.list`/`pane.get` scroll metrics (`offset_from_bottom`, `max_offset_from_bottom`, `viewport_rows`) and `pane.scroll` with `pane_id` and absolute `offset_from_bottom`. The handler clamps the offset and returns `pane_info`.
- A read-only request to the current focused pane returned a maximum offset of zero. Terminal history is not guaranteed: applications may own their scrollable content internally, which this API cannot scroll.
- Herdr injects `HERDR_SOCKET_PATH` into plugin commands. Its JSON API uses newline-delimited request/response messages.
- `pane.scroll` has no atomic focus condition. A fresh focus check before sending reduces the race but cannot eliminate a focus change between reading and dispatch.

## Decisions
- Use existing terminal scrollback rather than the previously prototyped wheel-routing API. No patched Herdr build, wheel events, PageUp, arrow-key or text fallback.
- Begin at 35% vertical deflection, stop within 20%, repeat at most every 100 ms, and move one to three history rows according to deflection. Horizontal and left-stick motion remain unused.
- Keep scrolling off the controller/voice loop. A single worker consumes the latest intent with no queued commands. Expired intent stops repeat when the controller loop stalls.
- Bind a gesture to its initial selected pane. Use `pane.list` only to bind the initial gesture, avoiding a scan of every pane on repeat. Before each update, read fresh `pane.get` for that identity, verify that same pane is focused, and derive the next offset from current metrics. Never retarget a held gesture. Clamp arithmetic using saturating addition/subtraction and the advertised maximum.
- At a scrollback boundary, send no mutation. With zero history or missing/invalid metrics, stop the gesture and give one concise diagnostic. Existing controls keep working.
- Buttons, menus, dictation, disconnects and errors stop a gesture; a tilted stick must return to neutral. Recheck gesture validity before socket reads and writes. An already-sent update may complete after suppression; the existing API cannot recall it or reject a concurrent focus change.
- Validate the returned `pane_info` identity and scroll metrics. Never retry a possibly delivered update automatically. Retain errors for the latest gesture even after release, without disturbing a successor.

## Work and validation
1. Replace SocketBackend's wheel calls with fresh metrics and absolute scrollback updates in `scrolling.rs`.
2. Add fake-socket regressions for up/down, current offset changes, clamping/overflow, boundaries, absent history, invalid metrics, focus changes, cancellation after lookup, and response identity/type validation. Observe assertion-red before implementation.
3. Preserve existing stick, worker, daemon ownership and RPC timeout/cancellation coverage. Production calls retain the 300 ms deadline; only large-response test fixtures use a two-second budget.
4. Update README, mapping and Controls help to describe terminal history and application-owned-history limitations.
5. Mutation-check changed guards, run the complete suite, obtain independent review and update draft PR #15. Hardware validation remains manual. Do not send synthetic input to live user panes.

## Verification of the revision
- Seven assertion-red regressions established the missing scrollback behavior before implementation: fresh offsets, saturation/clamping, boundary no-ops, no-history/invalid metrics, focus/identity validation, mutation-response validation, and cancellation after lookup.
- The final local suite passed 368 unit tests and four integration tests; one existing live-device test remains ignored.
- Eighteen changed-adapter guard mutations were killed by behavioral test failures and restored. The transport deadline/size and earlier controller/worker guard coverage remains in place.
- Read-only live probes confirmed the stock 0.9.1 schema, `pane.get` response shape, and zero-history behavior. No synthetic input or scrolling was sent to live user panes. Controller validation on real history remains manual.
