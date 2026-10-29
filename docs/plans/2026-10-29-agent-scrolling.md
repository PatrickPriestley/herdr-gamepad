# Experimental agent conversation scrolling

## Goal and authorization
Extend draft PR15 with right-stick PageUp/PageDown for Codex and Claude using released Herdr APIs. User requested planning then implementation after Fable5.1 adversarial review. Continue the existing isolated feat/joystick-scrolling worktree. Do not merge, release or install as part of this change.

## Probed facts
- Installed Herdr0.9.1 accepts pane/agent.send_keys. A fake-socket CLI probe captured pageup/pagedown strings. Matching v0.9.1 parser recognizes them. agent.send_keys additionally checks the live foreground job against the detected agent before writing; pane.send_keys does not.
- Official Codex rust-v0.165.0 transcript_view/input.rs handles page keys. app/owned_transcript.rs routes them to the transcript in owned-screen mode unless a modal, overlay or overriding binding takes priority. Claude fullscreen documentation explicitly describes PageUp/PageDown as half-screen conversation movement. These establish an experimental route, not a guarantee across app versions/renderers/custom bindings.
- Host history max0 is NOT a renderer detector. Agent identity alone is NOT a renderer detector. There is no reliable public modal/renderer-state contract.
- AgentInfo exposes agent_status and screen_detection_skipped (false is omitted); use those conservatively. These checks and live-process validation are not atomic with dispatch, so a concurrent dialog, focus, or process transition remains possible.
- Host scroll offset is not reset by agent.send_keys. Internal AttachScroll requires ownership and resize locks. Raw SGR mouse strings lack negotiated mode information. Reject both alternatives.
- Fable5.1 completed an adversarial review before implementation. Findings were checked against the v0.9.1 tag; artifacts: /tmp/gamepad-scroll-fable-review.log and /tmp/gamepad-scroll-review-resolution.md. Adopt live-agent validation, modal/state checks, host-offset handling, slower repeat, frozen identity, and honest validation limits. Reject footer scraping as state proof.

## Decisions
- Add scroll.agent_pages boolean, default false, with a Gamepad Settings toggle labeled experimental. This explicitly selects page-key behavior for known Codex/Claude panes; it never guesses renderer from scroll metrics. Users of classic renderers or custom page bindings leave it off. No new controller button/mode.
- Settings apply after the existing save-and-close boundary. Changing the setting cancels scrolling and requires neutral, preserving all unrelated settings.
- Bind terminal ID, agent label and agent-session metadata at gesture start. Validate them on fresh pane.get before each mutation. Agent-route binding changes stop until neutral. Host scrolling binds only the terminal and keeps its initial route even if agent metadata changes. Keep existing single-server focus semantics and document multi-client limitation; do not claim per-client binding.
- Agent route requires a matching focused agent.get response, allowed idle/working/done status, and no detection-skipped or launch-pending signal. Missing/malformed required metadata stops. Never toggle viewers, send arrows, Enter or raw mouse sequences.
- If host offset is nonzero, reset it once with pane.scroll and validate offset0, then recheck agent state before sending the page key. This intentionally returns the host view to the app when agent scrolling is selected.
- Send exactly one page key per request through agent.send_keys; repeat delays600/450/300ms according to tilt strength, measured from successful response. No queued repeats/retries. Preserve host-row speed and existing100ms worker/expiry semantics.
- Unknown agents and ordinary panes retain host scrolling; an agent-route failure never falls back to host scrolling or raw input. One concise notice per gesture. Existing stick-click dictation unchanged.

## Tasks and tests
1. Add settings/config/menu persistence with assertion-red default/strict-type/toggle/save-reload tests. Test off-on-off, unrelated comments/keys, and menu save boundary.
2. Add backend binding, agent validation and paced page dispatch with fake-socket assertion-red tests: both agents/directions, one key at full tilt, repeat deadline, host reset/recheck, identities/status/response validation, cancellation before every mutation, no fallback, setting off, and gesture reset.
3. Wire live settings to worker cancellation; test setting transition invalidates active intent and requires new neutral gesture.
4. Update README/Controls/mapping with opt-in instructions and renderer/modal/multi-client limits. Run locked full suite and source release build.
5. Mutation-check every new/touched guard, restore each mutation immediately. Complete Fable5.1 code review; probe findings and fix verified important issues test-first. Push existing draft PR15 with revised description when green.

## Acceptance limits
Automated socket fixtures prove routing and suppression, not application scrolling. Manual acceptance must cover installed Codex0.165.0 and Claude2.1.287 with long history, nonempty multiline draft, both directions, generation, modals/custom bindings, expanded panes, focus changes, app exit/replacement, nonzero host offset and setting off-on-off. Verify visible history movement AND unchanged draft/no submission. Keep draft until this passes; no synthetic input to existing user panes. Multiple attached clients sharing server-global focus are not supported for reliable local-controller targeting.

## Implementation evidence
- Read-only live agent.get probe confirmed agent_info shape, focused/status/terminal identity, omitted false flags, and consistent absent session metadata. No input was sent to a live pane.
- Reviewer-readable matching Herdr source is extracted at /tmp/herdr-0.9.1-scroll-review; do not inspect the patched checkout as if it were released.
- Assertion-red routing, pacing, identity, cancellation and settings tests were recorded before their implementation; socket tests run against disposable Unix sockets. Settings save/close/off-on-off tests and guard mutations are green.
- This build is an opt-in experimental implementation, not a claim that live-app acceptance has passed. Hardware and actual renderer/custom-binding acceptance remains the explicit draft-PR gate.

## Fable code-review follow-up
- Reproduced and fixed host-history interruption when agent metadata changes: only agent-input gestures require the full agent/session binding; all gestures retain terminal binding and their starting route.
- Reproduced and fixed misleading unsupported-method notices: Herdr error replies include the actual method and notices preserve it with bounded length.
- The settings-only commit is not being shipped alone; backend wiring is committed in the same feature branch before push.

## Final verification
391 unit tests and four integration tests passed; one existing live-device test remains ignored. The release binary builds successfully. All 44 initial backend/worker checks, eight settings checks and six review-fix mutation checks were caught and restored. Both Fable5.1 code reviews completed with their end marker. The final review found no blocking defect; transport errors retain their general diagnostics, and newly arriving agent session metadata may conservatively stop an agent gesture until recentered. Actual app/controller acceptance remains pending; PR15 stays draft.
