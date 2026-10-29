# Optional dictation preview

Accepted direction: Patrick Priestley, 2026-10-29. The user explicitly approved replacing separate intent/spec/plan PRs with this single implementation plan. PR #13 becomes the feature PR; no merge or release is authorized.

## Outcome and decisions

- `voice.preview_before_send = false` preserves automatic sending. Settings offers an On/Off toggle. A recording snapshots this choice; changing the setting never submits a retained draft.
- Preview contains the complete, scrollable transcript, destination pane, and controller-style labels. Cross/A sends once, Square/X deletes the whole draft, Circle/B hides and keeps it, and holding the configured voice button records a replacement.
- One in-memory draft survives dismissal and controller disconnect, but not daemon Stop/restart. Reopen through Actions → Review voice draft. While a draft exists, new speech replaces it only on successful nonempty transcription; failed/empty recordings preserve it. Replacement stays bound to the original target and never auto-sends, even when preview was subsequently disabled.
- Send requires the original pane to be focused and still contain the same agent kind, in an eligible state. Missing focus, blocked agent, unavailable server, or failed popup leaves the draft unsent. Never redirect it.
- Submission is asynchronous. Mark Sending before dispatch; ignore repeat sends. Clear only on acknowledged success. A failed dispatch retains the text in Outcome unknown and disables retry because delivery may have occurred. Delete/re-record is an explicit way to begin a new draft; no automatic retry.
- Popup acknowledgement must match the displayed revision before Send/Delete. Releases from buttons held before preview opening cannot activate it. Back always remains available. Only one controller popup owns input at a time.
- No word editing, rerunning stored audio, cross-restart draft persistence, joystick scrolling, or release publication. Do not add draft text to routine diagnostics.

## Tasks and files

1. Config/settings: `src/config.rs`, `src/settings.rs`, `src/menu.rs`, associated tests. Add typed boolean, default, persistence, and menu toggle.
2. Pure draft lifecycle: `src/engine.rs`, new `src/dictation.rs` and tests. Preserve automatic mode, pin target and mode, retain/re-record/discard drafts, model sending and unknown outcomes.
3. Preview runtime/presentation: new `src/voice_preview.rs` and tests; `src/feedback.rs`, `src/menu_render.rs`, `src/menu.rs`, `src/menu_runtime.rs`, `src/main.rs`. Reuse the existing popup publisher and viewer acknowledgement; add draft presentation and Actions reopening.
4. Daemon integration: `src/serve.rs` and tests. Route preview input before normal actions, keep recording/transcription completions flowing, send asynchronously, feed delivery outcomes back, and defer setting changes while voice is busy.
5. Documentation: README/CHANGELOG and this plan. Test complete suite and source build, mutation-check touched guards, then complete a different-family review with an explicit end marker before pushing PR #13. Install for controller testing only after software checks pass.

## Verification

Each scenario must first fail on an assertion: preview-on yields a retained draft rather than Prompt; default auto-send remains unchanged; settings round-trip; long Unicode transcript rendering; target drift; one-click send and duplicate suppression; successful delivery clears; failed delivery retains and prevents retry; failed/empty replacement retains previous text; Back/reopen; disconnect; stale popup acknowledgement; voice-button collision with Options; no popup/input fall-through. Break each new pure or safety-critical guard and verify a test catches it.

Existing seams inspected: Core currently captures the target on voice release, consumes Transcript immediately, and emits Prompt; serve applies Prompt without returning success to Core. Existing Publisher supports revision acknowledgement, viewer lifetime, and terminal width. These seams require explicit draft and delivery state rather than a toast-only preview.

Commands: `cargo test --locked`; `HERD_GAMEPAD_BUILD=source sh scripts/build.sh`; `git diff --check`. Installer and release packaging are unchanged. Hardware acceptance: enable preview, dictate, read/scroll, hide/reopen, record replacement, discard, send once, then disable and verify automatic send.
