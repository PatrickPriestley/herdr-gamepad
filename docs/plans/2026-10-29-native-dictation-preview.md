# Native dictation preview correction

The user rejected the separate transcript popup and reported that preview cannot be turned off. This correction supersedes the popup design in the earlier dictation plan.

## Probed facts and scope

Installed Herdr v0.9.1 `pane send-text` writes literal bytes without Enter. Its source was inspected in `/tmp/inline-herdr-panes.rs` (upstream `herdrdev/herdr`, tag v0.9.1). Unlike `agent prompt`, it does not submit. Therefore native preview will normalize transcript whitespace to spaces and remove terminal control characters before inserting. Agent identity, eligible state, and original focused pane are checked before insertion and before controller submission.

Herdr exposes no read/replace-composer API. Agent-specific deletion shortcuts are not reliable across supported agents. This correction implements the two reported problems: native input insertion, Cross to submit, and Off affecting subsequent recordings. Keyboard editing/deletion remains available in the native editor; automatic whole-draft deletion/replacement is deferred rather than emulated with interrupt keys. Recording again inserts additional text at the editor cursor.

## Implementation

- Replace popup draft orchestration with native input insertion and an acknowledged staged target. Never open a transcript popup or retain an invisible draft that forces preview back on.
- Snapshot preview mode at recording start. Saving Off controls subsequent recordings; it neither sends nor erases existing editor text.
- Cross sends Enter once after successful insertion, guarded against focus/agent/status drift. Circle returns to navigation. No popup-only review/delete controls remain.
- Keep automatic submission unchanged when Off. Clear stale staged controller state on configuration change. Insertion failures never claim readiness or emit Enter. Do not log transcript contents in preview mode.
- Remove obsolete popup rendering/menu entries/tests, retain setting persistence and tests, update README/CHANGELOG and PR #13.

## Validation

Assertion-red tests for native insertion instead of popup, Off after prior preview, single Cross submission, target drift, failures, and control-character normalization. Mutation-check changed guards; run suite/source build and different-family review before pushing. Install/restart the tested build for manual controller testing. No merge/release.

## Validation evidence

Both reported failures were captured by assertion-red tests before the fixes. The Off regression covers an acknowledged native insertion followed by saving Off and automatic submission on the next recording. The bridge tests verify no Enter on insertion, target checks before both operations, and propagation of insertion failure. A nonexistent-pane CLI probe confirmed `--` protects text beginning with a hyphen without delivering any input.

317 unit tests and four integration tests pass; one live test remains opt-in. All 57 targeted mutation checks were caught (44 core, five menu/settings, eight bridge/delivery). Forced source build and whitespace checks pass. Physical controller/microphone acceptance remains manual.

A further assertion-red regression caught menu ownership swallowing a Cross release. Resetting the submit latch on menu entry/exit restores the next fresh press; a mutation removing that reset was caught.

## Independent review follow-up

The completed GLM review identified pre-dispatch refusal recovery, stale staging after navigation, same-tick submission, Off notification, stale menu latch, and non-agent error wording. These were fixed with assertion-red tests and mutation checks. Refused-before-send is now distinct from uncertain dispatched outcomes; only the former permits retry. Controller navigation, menus, disconnect and Off discard staged submission. A fresh insertion cannot submit on a Cross event in the same tick.

The suggestion to strip printable escape-sequence residue was not adopted: the sanitizer guarantees no terminal control characters and preserves printable transcript text. A literal `[201~` is harmless text without ESC, as the regression test demonstrates. The input is a speech transcript, not terminal output to be interpreted or reconstructed.

The follow-up review identified transient snapshot failure sharing the menu disarm path. A separate mode/latch reset now preserves staged submission on temporary reads; the regression was observed assertion-red before the correction. Navigation-button cancellation is intentional even when there is only one tab/space: it returns control to the usual navigation bindings and avoids retaining an unexpected Enter capability. README describes cancellation by button press. Deferred-result toast concerns are not reachable in the synchronous dispatch path.

Final focused GLM review completed with `<<AGENT-REVIEW-END>>` and no important issues. Its comment-placement nit was corrected. All verified important findings are resolved; the two documented minor behavior choices remain intentional.

## Literal prefix correction

User testing revealed `-- ` prepended to every native transcript. The earlier nonexistent-pane probe proved only argument acceptance, not correct text parsing. A fake Unix socket with the installed CLI's ping handshake captured `pane.send_text.params.text` as `-- Testing voice.`. Herdr joins all trailing arguments as literal text; it does not interpret an option separator here. Remove the injected `--` argument, preserve user-spoken/literal leading hyphens, and regression-test the argv boundary.

The corrected fake-socket probe preserved ordinary text, `-- literal hyphens`, and `-leading` exactly. Both regression assertions failed before the fix; 318 unit tests and four integration tests pass afterward. Source build and independent GLM review completed successfully. No guards changed.
