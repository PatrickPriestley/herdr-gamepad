# Gamepad actions menu design

Accepted scope: conversation, 2026-10-28. Implementation base: 13c6b18.

One controller-owned popup provides space, worktree and pane actions, optional-context global settings, and diagnostics. The pure menu model owns navigation; the daemon owns operation lifetimes; the renderer acknowledges semantic revisions.

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

Split actions use the captured pane ID and an explicit source directory using the same rule. Focus the returned new pane after success. Toggle zoom acts on the captured pane. Rename provides an editable existing label and an explicit Clear label action.

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

Loading and command execution have explicit states. One mutation may be in flight; repeated input cannot submit it again. Navigation back is allowed during reads, with stale results discarded by session/request ID. During a mutation, show its target and Pending state. A timeout means outcome unknown: refresh state and explain it, never automatically retry creation, splitting, closing or removal.


See the implementation plan for fresh probe evidence, execution clarifications, verification and risks. Those clarifications take precedence where the original contract assumed separate pane focus.
