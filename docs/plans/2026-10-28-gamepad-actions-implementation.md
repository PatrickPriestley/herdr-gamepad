---
change_id: gamepad-actions
status: approved
accepted_by: Patrick Priestley (conversation)
accepted_on: 2026-10-28
implementation_base_inspected: 13c6b18
divergences: []
---

# Implementation plan: gamepad actions menu

## Outcome and boundaries

Operate common Herdr space, worktree and pane actions entirely with a controller through one plugin popup. Options/Menu opens this popup; close actions become a submenu. The current Navigate, Respond and Interact modes continue to control panes when the popup is closed.

This is a plugin menu backed by Herdr commands. It does not inject keyboard events into Herdr's native menus or into macOS. Settings in this menu configure Gamepad, not Herdr itself. Arbitrary repository browsing, custom worktree directory selection, arbitrary shell commands and full Herdr settings editing are outside this version.

The user authorized continuing this plan on 2026-10-28 and explicitly approved isolated Herdr API testing from Codex. No implementation code is embedded in the plan. The existing uncommitted close-label draft in `src/engine.rs` is preliminary and must be reconciled with the renderer work below; it is not a finished or installed feature.

## Evidence and existing architecture

- Inspected local `origin/main` at `1cba477`, the plugin manifest, engine, CLI adapter, renderer, daemon and CI workflow.
- Live Herdr CLI/server were probed at 0.9.1, protocol 22. `herdr api schema --output` succeeded; request and response fields were inspected from that output.
- Herdr exposes pane key input and workspace/worktree/pane operations. Its inspected API has no request for sending keys to the native client menus.
- `PaneInfo` exposes IDs, labels, `cwd` and `foreground_cwd`. `worktree.list` returns a source repository/checkout and worktree entries with paths and optional `open_workspace_id`.
- `worktree.remove` requires a workspace ID, and has a separate force option. The planned menu never supplies force or repository-trust overrides.
- The current `Core` owns a close-only state machine. `MenuUi` in `feedback.rs` launches one non-focused popup, sends revisions through a file, and requires a live renderer acknowledgement before confirmation.
- The current renderer prints plain text into a fixed 64-column, 12-row popup. `Herdr::apply` discards successful command results; it must return structured results for actions that create or select resources.
- The current CLI runner can wait up to 10 seconds on the event-loop thread. Menu commands must move off that thread to keep navigation and cancellation responsive.

These probes establish available fields and commands, not every runtime semantic. Step 1 verifies creation/removal behavior in disposable fixtures before command contracts are finalized.

## User-facing contract

### R1: Entry and controls

Options/Menu opens the top-level menu. If Options/Menu is configured as the voice button, retain the existing L3 fallback. The menu button closes the whole popup; Circle/B goes back one level and closes it at the root. Create/Share/View also closes the whole popup.

D-pad Up/Down selects rows; Left goes back and Right opens a selected submenu. Cross/A activates the selected row once. Selection stops at list boundaries. Hold-to-repeat applies only to D-pad movement after a delay; activation and destructive confirmation never repeat. Shoulders and triggers do not switch spaces while a menu is open.

Opening enters Navigate and suspends pane routing. Closing returns to Navigate; an input used to close or dismiss a menu must never also reach a pane. Opening while recording or transcribing is refused with a short explanation, as it is today. Voice is unavailable while the menu is open.

### R2: Scope and target identity

Capture the focused workspace, tab and pane IDs when opening. Show a human label plus ID in the header and the repository/path on relevant screens. Worktrees use that workspace's repository. Pane actions target that pane. Global settings and logs are explicitly labelled Global.

The popup keeps underlying pane focus unchanged. Revalidate target IDs immediately before a mutating action. If external focus changes, a target disappears, or a snapshot fails, close the menu and consume the current input. Do not silently retarget. Operations already submitted remain pending until their result is reconciled; a focus change is not evidence that the operation failed.

### R3: Menu tree

| Screen | Rows and behavior |
| --- | --- |
| Main | New space; Worktrees; Pane actions; Close…; Settings; Logs; Controls |
| New space | Show captured working directory; suggested editable name; Create and switch |
| Worktrees | Create worktree; Open existing; Remove worktree |
| Pane actions | Split right; Split down; Toggle zoom; Rename pane; Close pane |
| Close… | Close captured pane; Close captured space |
| Settings (Global) | Button labels; Voice button; Maximum recording duration; View voice configuration |
| Logs (Global) | Recent log lines inside this popup; Refresh; Back |
| Controls | Scrollable mapping, reflecting current label and voice-button settings |

Disabled rows remain visible with a concise reason. Missing repository context disables worktrees, but not global settings/logs. Missing pane context disables pane/space actions. Do not fall back to the daemon's inherited directory or caller pane.

### R4: Space and pane operations

New space uses the captured pane's nonempty absolute `foreground_cwd`, falling back to `cwd`. If neither is available, explain why creation is unavailable. Use an editable suggested name such as `Space 20261028-0905`; pass the explicit directory and name to workspace creation, then focus the returned workspace ID after success.

Split actions use the captured pane ID and an explicit source directory using the same rule. Use the split command’s `--focus` flag: Herdr cannot focus an arbitrary non-agent pane by ID afterward. The focus side effect belongs to the submitted mutation and cannot be cancelled after dispatch. Toggle zoom acts on the captured pane. Rename provides an editable existing label and an explicit Clear label action.

Closing a pane or space shows a confirmation screen with its label, ID, and the fact that running processes stop. Cancel is initially selected. One Cross/A press on the Confirm row executes once. Closing a space does not delete its Git worktree. If Herdr requires a grouped workspace close, display that refusal; do not escalate to closing the whole group automatically.

### R5: Worktree operations

Use `worktree list --workspace <captured-id>` to resolve the repository and source checkout. Present the repository name/path in every worktree screen.

Create: suggest an editable unique branch name `gamepad/YYYYMMDD-HHMMSS`. Resolve and pin the source checkout's HEAD commit when the creation form loads. Show the branch, base commit and repository before Create. Validate the branch with Git's ref-name rules and reject existing names. Use an explicit workspace, branch and pinned base; let Herdr choose its normal worktree location. The final location is displayed from the successful response. On success focus the returned new workspace.

Open existing: list existing worktrees for this repository, with branch (or detached indication), path and open-space status. Select by returned path. If already open, focus its workspace; otherwise invoke worktree open and focus the returned workspace. Do not match entries solely by a display name or list index.

Remove: list linked, non-bare worktrees. Removal is offered only for an entry with a live associated workspace ID. Closed entries explain that they must be opened first; the primary checkout cannot be removed. Confirmation names the selected path, branch and workspace, and explicitly distinguishes removing the checkout from merely closing a space. Cancel is selected initially. Re-read the entry and verify its path/workspace association before removal. Do not pass `--force`; show Herdr's refusal for protected or dirty worktrees without retrying destructively.

The default repository is fixed for a menu session. Switching to another repository means closing the menu, switching space, and reopening it in this version.

### R6: Names can be entered with the controller

Use one reusable text editor for space names, branch names and pane labels. Show an editable buffer and a small on-screen character grid. D-pad moves within the grid; Cross/A inserts the selected character or activates its Done cell; Square/X deletes one character; Triangle/Y switches lowercase, uppercase and symbol pages; Circle/B cancels the edit and restores the prior value.

Include slash, hyphen, underscore, digits and space in the grid. Validate the completed buffer for the field rather than silently rewriting it. Name/label buffers have an explicit displayed length limit; branch validation additionally uses Git. Suggested names allow the common create path without typing. Arbitrary file path editing and speech-to-text in these forms are deferred.

### R7: Settings and diagnostics

Button labels offer Both (default), PlayStation and Xbox. Both shows glyph/name and the Xbox equivalent; never rely on color or a special icon font. This is an explicit preference, not controller-name guessing.

Voice button offers R3, L3 and Options/Menu, using the existing conflict fallback. Maximum duration uses presets 15, 30, 60, 120 and 300 seconds; display an existing custom value without changing it until the user selects a preset. Model path and language are shown read-only with their configuration location.

Save only changed keys, preserve other configuration values/comments using a format-preserving TOML editor, and replace the file atomically. Detect a configuration file changed externally since the form was loaded; reload and ask the user to retry instead of overwriting it. Apply saved runtime settings together after exiting the menu. Failed writes leave the previous settings active.

Logs and Controls render in the same popup so the controller can scroll and return. Refresh logs on request, not continuously. Display errors inline; avoid queued notification popups.

### R8: Presentation and operation state

Use a header, breadcrumb, highlighted selection, optional detail area, and fixed controller legend. Format close controls with `□ Square`, `△ Triangle`, `✕ Cross`, `○ Circle` and readable Xbox equivalents according to the label setting. Show optional cancel shortcuts in one compact footer, not three identical action rows.

Use standard Unicode glyphs plus names, with a text fallback. Add terminal-width-aware clipping and scrolling; never clip the selected action or its confirmation target without an expandable details view. Escape control characters in labels, paths, errors and logs before rendering. Hide the terminal cursor while the viewer is active and restore it on exit.

Loading and command execution have explicit states. One mutation may be in flight; repeated input cannot submit it again. Navigation back is allowed during reads, with stale read results discarded by session/request ID; mutation outcomes survive dismissal. During a mutation, show its target and Pending state. A timeout means outcome unknown: retain the global mutation slot, refresh state and explain it, never automatically retry creation, splitting, closing or removal. Release the slot only after reconciliation proves the outcome. If identity cannot be established, keep mutations disabled and show the unresolved operation.

## Implementation structure and files

| File | Responsibility |
| --- | --- |
| `src/menu.rs` (new) | Pure menu state, screens, selection, context binding, forms, confirmations, request/result identities and view model |
| `src/menu_render.rs` (new) | View-model-to-terminal rendering, layout, clipping and button legends |
| `src/menu_tests.rs`, `src/menu_render_tests.rs` (new) | Menu contracts and rendering edge cases |
| `src/engine.rs`, `src/engine_tests.rs` | Replace close-only state with menu integration; retain existing pane/agent controls |
| `src/herdr.rs`, `src/herdr_tests.rs` | Parse contextual details and typed read/mutation results; construct explicit-target commands |
| `src/feedback.rs`, `src/feedback_tests.rs` | Generalize popup session, live-view acknowledgement, teardown and error presentation |
| `src/menu_runtime.rs`, `src/menu_runtime_tests.rs` (new), `src/serve.rs`, `src/serve_tests.rs` | Daemon-owned operation coordinator, result dispatch, settings activation and input routing |
| `src/menu_worker.rs`, `src/menu_worker_tests.rs` (new) | Bounded asynchronous reads, popup launch, explicit-target mutations and reconciliation; cancellation and joining |
| `src/config.rs`, `src/config_tests.rs` | Label preference, validated updates, atomic persistence and external-edit detection |
| `src/main.rs`, `herdr-plugin.toml`, `tests/manifest.rs` | Add general menu viewer entrypoint/title/size; retain old close-menu command as a compatibility alias |
| `Cargo.toml`, `Cargo.lock` | Add direct terminal-width and format-preserving TOML dependencies if existing dependencies do not supply these contracts |
| `README.md`, `CHANGELOG.md` | New menu controls, scopes, text entry, limits and unreleased change notes |

Keep daemon/controller input ownership in one process. The popup remains a display process, not a second controller reader. Reuse the revision/acknowledgement and viewer lease mechanism. A typed view model replaces `close_view()` strings; confirmation requires acknowledgement of the exact displayed target/revision. Consume one press on entering a new screen; it cannot also activate a row on that screen.

## Order of work and acceptance checks

1. **Confirm the adapter contracts (R2–R5).** Refresh the base branch before starting implementation; inspect any newly changed APIs. Probe list/create/open/remove, pane split/rename and explicit focus behavior in a disposable repository and a named test session, never the user's working panes. Record actual response/error fixtures. Include a dirty-worktree removal refusal and an unknown-outcome case. Resolve any API mismatch in this plan before implementing a substitute.
2. **Build the pure menu foundation (R1, R2, R3, R8).** Add the menu/view types and root/submenu navigation. Assertions must first fail because Options currently opens the close-only menu, D-pad cannot select rows and back navigation does not exist. Cover root/submenu back, mode return, voice conflicts, scope loss, stale result IDs, empty lists and selection boundaries.
3. **Build the renderer and text editor (R6, R8).** Exercise narrow/wide viewports, long Unicode labels, malicious terminal escape sequences, visible selection/legend and text edit cancellation. Assertions must require newly rendered structure and edited buffers, not merely absence of output. Integrate a single popup with current-revision acknowledgement. Require a stale acknowledged screen to block activation until the new screen is drawn.
4. **Add read/action execution and pane/space flows (R2, R4, R8).** Introduce the asynchronous worker and typed command outcomes. Verify every generated command targets the captured ID/directory, a read cannot execute a mutation, rapid confirmations submit once, errors stay in the menu and uncertain outcomes never auto-retry. Exercise new-space creation, both split directions, zoom, rename, clear label, and pane/space confirmation cancellation in disposable fixtures.
5. **Add worktree flows (R5, R6, R8).** Verify repository scoping, pinned base commit, invalid/duplicate branch rejection, existing-worktree selection by path, live-space association, disabled primary/closed removal targets and confirmation identity. Integration checks must prove a newly created checkout is associated with the expected repository/base and that a dirty removal is refused. Never silently add force/trust flags.
6. **Add settings, logs and controls (R7).** Require persisted settings to survive reload, preserve unrelated keys/comments, reject external-edit conflicts and failed writes, and activate only at the defined menu exit boundary. Verify menu/voice fallback after each voice-button change, bounded log scrolling and current labels in Controls.
7. **Integrate and review (all requirements).** Update docs/manifest and resolve existing close-menu tests whose expectations change. Run focused tests for each step, then the existing CI checks. Mutation-test changed pure menu/engine/config logic and cover every meaningful survivor. Obtain an independent review from a different model family, validate findings, and perform the hardware walkthrough below. Submit an implementation PR only after these checks, with the plan and any documented deviations.

Follow the repository's failing-test-first rule during implementation. Each new scenario must demonstrate a currently failing positive assertion before its implementation. No tests or application changes are executed as part of drafting this plan.

## Verification and completion

Use the repository's current CI commands: `cargo test --locked`, `python3 -m unittest discover -s tests -p 'test_release*.py'`, `sh tests/build.sh`, and `sh scripts/build.sh`. Also run `cargo check --locked` and `git diff --check`. The earlier formatting check found repository-wide baseline differences; do not reformat unrelated files to make this feature pass. Confirm the project's formatting policy before adding a new formatting gate.

Mutation coverage targets changed pure modules, with guards removed or inverted to prove that scope, acknowledgement, pending-operation, validation and confirmation checks have tests that fail. Review focuses especially on an input reaching a pane after dismissal, a late result operating in another space, and a destructive operation targeting a different checkout.

Manual walkthrough with a DualSense and Xbox label setting: open/close the menu; navigate every screen; enter/cancel a name; create/switch space; split/rename/zoom a pane; create/open a disposable worktree; refuse dirty removal; remove a clean disposable worktree after explicit confirmation; cancel close actions; change settings; scroll logs; disconnect/reconnect; change focus with the mouse while the menu is open; exit the renderer unexpectedly. Verify icons and targets at the user's normal terminal size. Repeat relevant build/CI checks on macOS ARM and Intel.

Completion means the full menu is controller-operable, every action clearly identifies its scope, failures remain recoverable, existing Navigate/Respond/Interact and dictation flows pass their regression checks, and the reviewer/hardware walkthrough has no unresolved important findings. Installation into the user's live plugin and a release are separate steps after review; drafting or approving this plan does not publish a release.

## Risks and alternatives

- **Highest risk: stale scope or delayed input causes the wrong action.** Use captured IDs, a fresh pre-action check, revision acknowledgement, one in-flight mutation, and session-scoped results. Herdr may not provide an atomic compare-and-act contract; document the remaining external-change race instead of claiming it is eliminated.
- **Worktree removal can stop processes and remove files.** Use a separate explicit confirmation, exact path/workspace association and non-force API operation. Closing a space and removing a checkout have different screens and wording.
- **Names require text entry.** A reusable character grid keeps the feature controller-operable; suggested names make it optional for common actions. Voice naming and arbitrary path selection would add routing complexity and are deferred.
- **Popup focus/size can vary by client.** Preserve no-focus ownership and test actual rendering/focus behavior in the disposable session and hardware walkthrough. If the host cannot display the proposed layout, adapt layout/scrolling, not target routing.
- **Broad settings editing would increase scope.** Limit editable settings to label style, voice button and duration; show model/language read-only.
- Rejected: macOS-wide keyboard injection, pretending to control native Herdr menus, invoking shell strings, automatically trusting repositories, automatically forcing removals, and rebuilding Herdr's entire settings UI.
- Retaining a separate close-only entry button adds binding conflicts. The selected design puts close actions inside Options/Menu and uses Circle/B consistently for back.

## Approval boundary

Scope accepted in the conversation on 2026-10-28. Implement on feat/gamepad-actions from 13c6b18. Reconcile the earlier uncommitted close-label draft as part of the new renderer; retain its readable names and glyphs. Live installation and publishing remain separate from implementation.


## Execution clarifications (2026-10-28)

- A daemon-owned operation slot survives popup dismissal. Discard stale read results only; retain and log every mutation outcome. A new popup cannot submit another mutation while one is pending.
- Auto-focus a created/opened workspace only while its originating session still owns the captured context. On dismissal or external focus change, reconcile without focus changes or reopening the popup.
- Herdr 0.9.1 has no arbitrary non-agent pane-focus CLI: `agent focus <non-agent-pane>` returns `agent_not_found`. Split therefore uses `pane split --pane <captured> --direction right|down --cwd <explicit> --focus` as one mutation. Its focus side effect is part of the submitted operation and cannot be recalled after dispatch. This is the documented external-change race, not a later focus retry.
- Context is optional: a successful no-focus snapshot allows global settings, logs and controls. Snapshot failure differs from no focus. Any subsequent focus identity change dismisses the menu instead of acquiring a new target.
- Renderer revisions represent semantic state, including IDs/selection, not just clipped text. Require current acknowledgement for activation and edits; Right only opens submenus and never executes a leaf action.
- Settings use explicit Save and Discard. Unsaved edits are discarded on exit. Successfully saved settings activate together on any exit path, including renderer disappearance. Core, daemon routing/configuration and legends change together. A dismissal press/release is consumed across rebinding.
- Menu context reads and popup launch must not block controller cancellation. Mutation validation runs again in the worker immediately before dispatch. Distinguish cancellation before dispatch from unknown outcomes after dispatch; never auto-retry unknown mutations.
- The draft contains behavioral contracts rather than embedded implementation code. Build/test/mutation verification applies to each implemented task; do not claim a nonexistent code listing was compiled.

## Fresh probe evidence

Isolated server: `gamepad-actions-probe-20261028`, Herdr 0.9.1. Only disposable repository `/tmp/gamepad-actions-probe-repo` and linked checkout `/tmp/gamepad-actions-probe-linked` were used.

- `workspace create --cwd ... --label ... --no-focus` returns `workspace`, `tab`, and `root_pane` with explicit IDs and absolute cwd.
- `worktree list --workspace ...` returns source `repo_key`, `repo_root`, `source_checkout_path`, and entries with path and optional `open_workspace_id`.
- `pane split --pane ... --direction right --cwd ... --no-focus` returns `result.pane`; rename and `--clear` return pane info.
- `worktree create --workspace ... --branch gamepad/probe --base <pinned commit> --no-focus --path <scratch>` returns the workspace and checkout association.
- Removing a checkout containing an untracked file without force fails with `dirty_worktree_requires_force`. No forced removal was attempted.
- `agent focus` on a non-agent pane fails with `agent_not_found`; creation-time focus is required for splits.

The source checkout spelling and returned canonical path can differ (`/tmp` versus `/private/tmp`); compare canonical filesystem identity for repository checks, while preserving server-returned paths for operations.

Additional successful probes: closing a linked workspace preserves its checkout; opening that same path returns `worktree_opened` with a new workspace ID. Clean removal returns `worktree_removed` with `forced: false`. Split-down with `--focus` returns a focused new pane; explicit workspace focus returns workspace info. A controlled response-loss wrapper completed one workspace creation then withheld its response until the caller timed out; reconciliation found exactly one created workspace and no retry was issued (`/tmp/gamepad-menu-outcome-probe.py`).

Independent design review: Claude Sonnet completed a read-only review (`/tmp/gamepad-menu-design-review.txt`). Accepted clarifications above resolve delayed focus, mutation lifetime and settings activation. A successful self-focusing operation may close the menu; its result must still be reconciled. Opening-time snapshot failure refuses opening (it is not a successful empty context). Settings conflict reloads current disk state, discards unsaved edits and displays a retry message. Atomic replacement plus a cooperative writer lock avoids partial writes and competing plugin writers, but cannot guarantee compare-and-swap against an unrelated editor that ignores that lock; a final content comparison narrows that residual race.

Default-location probe also passed: omitting `--path` created the scratch repository’s linked checkout under `~/.herdr/worktrees/gamepad-actions-probe-repo/gamepad-default-location-probe`, with the returned path and workspace association. No implementation should infer that path; always use the response.

Settings library probe: toml_edit 0.22.27 in `/tmp/gamepad-toml-probe` showed direct value assignment drops the inline comment. Copying the old value decor to the replacement preserved it and unrelated table values. Implement and test that preservation explicitly.

Runtime decomposition after foundation review: keep Core for normal modes and voice; intercept menu entry before synchronous pane routing. A daemon-owned coordinator owns Opening/Open states, the persistent operation slot and committed settings. Read/popup jobs remain independent of the single mutation worker; completions carry typed resource IDs and committed config separately from display messages. Every exit shares one path. A late committed settings result must update runtime even if the original Menu was dropped. Preserve renderer lease/flush-before-ack tests and migrate close routing tests to the new menu path.

Foundation quality review clarifications: successful structural operations (create/open/remove/close) consume their screen and dismiss the popup; only a fresh session can submit a new structural action. Compute guarded follow-up focus eligibility before delivery of that success. A refreshed repository response must retain the original opaque repo_key; every worktree operation carries that key for pre-dispatch validation. Unicode rendering measures complete grapheme clusters consistently, and directional control characters display visibly rather than reordering confirmation targets.

## Execution progress: foundation

Pure menu and renderer implemented, with frozen typed effects, optional context, semantic revisions, matching key releases, character-grid entry, versioned settings results, and persistent operation-slot display. Spec review passed after fresh Create-form HEAD and shared scroll geometry fixes. Independent Claude quality review findings were reproduced in failing assertions, then fixed: legend composition, grapheme width, consumed structural confirmations, and bidi controls.

Mutation evidence (new pure modules): final full sweep 359 mutants = 338 caught, 17 unviable, 4 survivors; added assertions for the surviving Pending→Unknown display/revision transition and reran all four against unchanged production source: 4 caught. Combined final result: 342 caught, 17 unviable, zero survivors; no equivalent-mutant exclusions. Evidence: `/tmp/gamepad-menu-mutations-round4/mutants.out` and `/tmp/gamepad-menu-mutations-final-iterate.log`.

Focused tests pass. Full suite at this checkpoint: 212 pass, two pre-existing feedback wording failures from the earlier visual draft remain; integration must migrate them. Runtime/adapters/settings persistence are not yet implemented.

Foundation quality re-review approved with no important findings: `/tmp/gamepad-menu-foundation-quality-rereview.txt`. Root independently reran the focused suite successfully after commits 4a8d4c5/3142659. Runtime integration now in progress.

## Execution progress: integration

The daemon now owns bounded asynchronous read and mutation workers, opening/open sessions, a persistent operation slot, and deferred settings activation. The passive popup uses typed semantic revisions, actual terminal geometry, a live viewer lease, and flush-before-acknowledgement. Settings preserve comments and unrelated values, detect external changes, and refuse symlink destinations without replacing the link or target.

Specification review passed after fixing dispatched-operation cancellation, disk-versus-active settings baselines, shared detail width, and expandable retained operation results. Successful structural actions still dismiss their originating menu; reopen Actions to inspect the complete last result, including the returned worktree path. This presentation limitation is documented in README. Literal pane label `--clear` is refused because a live CLI probe showed that `--` is retained as label text rather than acting as an argument delimiter.

Real adapter smoke passed in the approved isolated session (`/tmp/gamepad-runtime-live-smoke-3.log`): space creation, rename/clear, zoom, split, worktree creation, closed/already-open worktree selection, dirty removal refusal, and clean non-forced removal. Actual response fixtures are recorded in `/tmp/gamepad-live-adapter-responses.log`. CLI command replies differ from event schemas: worktree metadata is nested under `result.workspace.worktree`, checkout entries under `result.worktree`, removal returns top-level `result.path`, and workspace focus returns `workspace_info`.

Known missing-target, dirty-checkout, and grouped-close refusals are definitive failures and permit deliberate retry. Internal errors and other unproven outcomes remain unknown, without automatic retry. A probe of a duplicate checkout returned `worktree_create_failed`; that broad error is not treated as proof that no side effect occurred. Isolated test root workspaces that refuse individual close remain for cleanup by stopping only the named test server.

Independent Claude Sonnet review identified structured-refusal handling, horizontal-repeat boundaries, and symlink replacement. All findings were reproduced and fixed with regressions, including held horizontal input after leaving the editor. Final independent approval: `/tmp/gamepad-menu-runtime-quality-final.txt`.

Final root verification used a separate Cargo output directory to avoid reusing mutation artifacts: 272 unit tests and four integration tests passed; the one ignored live test passed separately as described above. `cargo check --offline --locked`, `sh scripts/build.sh`, `sh tests/build.sh`, all 12 release Python tests, version/mapping CLI smoke, and `git diff --check` passed. The isolated Herdr server is stopped, and `git worktree list` in its scratch repository confirms that no linked test checkouts remain.

Mutation evidence: adapter/runtime 205 mutants = 193 caught and 12 unviable, no survivors; final repeat/terminal-size guard sweep 17/17 caught. Feedback 68 = 64 caught, two unviable, two non-load-bearing survivors (disjoint flock OR/XOR flags produce identical arguments; strict versus inclusive five-second startup deadline). Settings 34 = 28 caught, five unviable, one proven-equivalent flock flag expression. Both settings symlink checks were also deleted separately and caught. Relevant logs: `/tmp/gamepad-runtime-mutations-v4.log`, `/tmp/gamepad-final-guard-mutations.log`, `/tmp/gamepad-feedback-mutations.log`, and `/tmp/gamepad-settings-symlink-final-iterate.log`.

Implementation and software verification are complete. Physical controller walkthrough and Intel execution have not been performed; they remain the acceptance checks before installation/release. No implementation PR was submitted because this plan places submission after those checks. Nothing has been installed into the user's live plugin or released.

Final pure-menu integration audit: seven generated changed-line mutants = four caught and three unviable, with no survivors. Separate deletion probes caught active-settings assignment, safe settings-summary redaction, retained-result details, and each Create/Open/Remove frozen repository-root field. The latter three now have explicit emitted-operation assertions. Evidence: `/tmp/gamepad-menu-final-diff-mutations.log` and `/tmp/gamepad-menu-root-final-green.log`.

## Follow-up review fixes

The branch review identified a late-save voice handoff bug and controller-label inconsistencies, plus positional coupling between row labels and their actions. Saved configuration now remains pending while recording or transcription is active and activates on the first closed-menu, voice-idle tick. Controls and legends share one controller-label resolver. Internal typed menu rows carry their action and submenu eligibility together; only presentation data crosses the popup IPC boundary.

Regression assertions were observed failing before implementation. Coverage includes successful and failed transcription completion, all nine label-style/voice-binding combinations, and reordered pane/worktree rows. The final software checks passed: 279 unit tests, four integration tests, 12 release tests, build/installation checks, cargo check, and diff whitespace checks. The existing live test remains ignored in the ordinary suite. Three runtime guard mutants and 14 menu/renderer mutants were caught; scratch mutations did not modify the branch. Independent OpenCode Go GLM review completed using the patch and surrounding sources. Its two presentation concerns were checked against rendered output: editor detail metadata is not displayed, and the unshortened Both footer is 70 columns, so the compact wording preserves Close at 64 columns. Neither concern reproduced a user-facing defect; the recording handoff and typed action routing had no reported defects. Evidence: `/tmp/gamepad-fixes-glm-packet-review.log` and `/tmp/gamepad-review-render-evidence.log`. No live installation or release was performed.

### Worktree availability follow-up

- Reproduced on space w1M: `worktree list --workspace w1M` returned `not_git_worktree`, while `--cwd /Users/patrickpriestley/Code/herdr-gamepad` succeeded. The space began in `~/Code`; the focused pane was in the repository.
- Preserve workspace-first resolution; only that specific error permits fallback to the captured pane's absolute directory. Resolve again before mutation and retain repository identity/root checks. Create/Open dispatch with the resolved root.
- Regression failed with the reported error before implementation. Added fallback, error preservation, missing-directory and mutation-target coverage.
- Validation: 282 unit and four integration tests passed; five targeted guard mutations were caught. A temporary read-only test using the real CLI resolved the repository for w1M successfully; removed the machine-specific probe afterward.
