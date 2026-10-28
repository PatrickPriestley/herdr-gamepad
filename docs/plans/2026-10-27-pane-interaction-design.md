# Controller interaction with non-agent panes

## Intent

Use the controller to navigate applications such as PR Tracker: select categories, move through rows, open details, and return, without requiring the pane to contain an agent.

## Probed facts

- An isolated raw-terminal receiver captured `herdr pane send-keys <id> up down left right enter space esc` as CSI A/B/D/C, CR, ASCII space, and ESC. Probe evidence: /tmp/gamepad-pane-probe/facts.md.
- The live PR Tracker pane has no `agent` field and reports status unknown.
- The installed PR Tracker help and its primary source `src/ui.rs` support left/right categories, up/down rows, Space details, Enter open, and Escape back.
- The current gamepad adapter sends Respond keys through `herdr agent send-keys`; non-agent interaction needs the pane command instead.

## Decision

Keep Navigate for choosing panes. Cross (Xbox: A) enters Interact on a focused non-agent pane, consuming that first press. In Interact, D-pad sends arrows, Cross sends Enter, Square (Xbox: X) sends Space, and Circle (Xbox: B) sends Escape. Create/Share (Xbox: View) returns to Navigate. This works with any terminal application supporting those keys; it does not introduce PR Tracker-specific actions.

Shoulders and triggers retain space/tab navigation and leave Interact. Triangle retains next-agent navigation and leaves Interact. Options retains the existing close menu. Voice retains its existing agent-only delivery. Mapping and README explain both Respond and Interact with familiar controller names.

Automatic interaction on focus would remove a press but make D-pad pane navigation dependent on the focused process. Per-application bindings would expose more shortcuts but require profiles and app detection. Explicit entry is the smallest predictable extension of the existing two-mode behavior.

## Routing and safety

Add a distinct Interact mode and pane-key action. Existing agent-key routing and blocked-agent acknowledgement stay unchanged. Interact requires a nonempty explicit pane ID and binds to it. Before handling an Interact key or navigation button, missing focus, changed pane, or an agent appearing exits the mode and consumes that press. Disconnect resets to Navigate. Entry never sends Enter to a shell or application. Menu and voice retain their higher-priority handling.

## Verification

Write positive failing assertions first for entry, arrow/Enter/Space/Escape routing, exit, focus/agent changes, invalid IDs, disconnect, and unchanged agent behavior. Verify the pane adapter's argv and mapping. Assemble the complete plan in scratch, build and test, mutation-test changed pure logic, and delete each new guard to prove coverage before committing the plan. Apply through a fresh implementer, spec review, quality review, and different-model final review. Validate real pane input using an isolated receiver; PR Tracker interaction remains a user hardware check before claiming end-to-end controller validation.
