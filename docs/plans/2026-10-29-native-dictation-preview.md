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
