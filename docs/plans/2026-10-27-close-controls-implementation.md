# Close Controls Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Close the selected pane or space through an explicitly confirmed controller action.

**Architecture:** The pure engine owns a close menu bound to a captured pane and workspace pair. Existing CLI actions perform explicit-ID close requests, and existing notifications display selection, confirmation, cancellation, and failures. The daemon cancels close intent if snapshot acquisition fails.

**Tech Stack:** Rust 2021, existing gilrs button events, Herdr CLI, cargo test and cargo-mutants.

## Verified design and probes

See `2026-10-27-close-controls-design.md`. CLI probes used only newly created disposable
panes. Close requests return acceptance before topology necessarily changes; no completion
message is emitted. Workspace close must omit `--group`.

## Task 1: Pure close selection, confirmation, and CLI dispatch

Files: `src/engine_tests.rs`, `src/engine.rs`, `src/herdr.rs`.

1. Apply the final engine-test changes below first. They add the `closing` scenarios and
   replace old tests asserting unused Menu behavior with unused-left-stick tests.
2. Run `cargo test --locked closing`. On the original implementation all ten scenarios
   fail with positive assertions: Menu returns no action instead of showing close actions.
3. Apply the engine and Herdr adapter changes. Open state captures both nonempty IDs;
   choosing X/Y never closes; A without selection prompts; selected A takes the state and
   emits exactly one explicit-ID close. B/View/menu cancels. A stale or absent focused
   pair cancels. Voice is suppressed while open, and active dictation refuses opening.
4. Run `cargo test --locked closing` and `cargo test --locked`.
5. Commit: `feat: add confirmed controller close actions`.

## Task 2: Fail closed when a snapshot cannot be read

Files: `src/serve_tests.rs`, `src/serve.rs`.

1. Add `snapshot_failure_disarms_pending_close` from the patch, initially calling the
   existing helper as `on_snapshot_failure(Input::Press(Btn::A), "offline")` (without
   `&mut c`). Keep its subsequent positive expectation of the Respond-mode toast.
2. Run `cargo test --locked snapshot_failure_disarms_pending_close`. It fails because
   the old helper leaves the core armed and the next A emits `ClosePane("w5:p1")`.
3. Change the helper to receive `&mut Core` and call `cancel_close()` before preserving
   its existing routing behavior. Update its caller and tests as shown in the patch.
4. Add the close feedback test from the patch: successful CLI acceptance emits no
   completion toast; failures emit exactly one existing Herdr-refused notification.
5. Run `cargo test --locked`; commit `fix: disarm close actions when context is unavailable`.

## Task 3: Explain controls in the mapping and README

Files: `src/serve_tests.rs`, `src/serve.rs`, `README.md`, `CHANGELOG.md`.

1. Add `mapping_explains_close_selection_confirmation_and_voice_collision` first.
2. Run `cargo test --locked mapping_explains_close`; it fails because CLOSE ACTIONS
   and the selection/confirmation bindings are missing.
3. Apply the mapping text, README, and changelog changes below. Menu collision uses
   left stick click only when voice is configured to Menu. Keep the mapping on demand.
4. Run `cargo test --locked`, `cargo build --locked`, and the mutation commands below.
5. Commit: `docs: explain pane and space close controls`.

## Verification before this plan was committed

The final patch was assembled in `/tmp/herdr-gamepad-close-plan` from base `e96f7b5`.
- Baseline: 119 unit + 4 integration tests pass.
- RED: 10 close-flow assertions, stale-close snapshot regression, and mapping assertion
  each observed failing before the corresponding implementation.
- GREEN: 132 unit + 4 integration tests pass; `cargo build --locked` passes.
- `cargo mutants --file src/engine.rs --jobs 4 -- --bin herd-gamepad`:
  123 mutants, 107 caught, 16 unviable, zero missed/timeouts.
- `cargo mutants --file src/serve.rs --file src/herdr.rs --re 'on_snapshot_failure|argv_for'
  --jobs 4 -- --bin herd-gamepad`: 6 mutants, 5 caught, 1 unviable, zero missed/timeouts.
- Manual guard removals: menu dispatch, voice suppression, menu trigger, active-voice
  refusal, nonempty IDs, pinned focus, cancellation buttons, cancellation reset, menu
  persistence, Respond exit, and one-shot state consumption all fail assertions.
- Guard deletion exposed that the voice-suppression test needed the actual empty
  snapshot used on a voice press; this is corrected in the final test.

After implementation, run fresh spec and quality reviews, then the different-family
review required by AGENTS. Probe the actual adapter only against newly created disposable
terminals. Do not close existing user panes or claim hardware validation. The user can
exercise Menu → X/Y → B first, then confirm on a disposable pane after installation.

## Verified final patch

This patch is the complete final change, grouped by file. Apply test portions first as
in the steps above; the snapshot test uses the final helper signature in this patch.

```diff
--- a/src/engine_tests.rs
+++ b/src/engine_tests.rs
@@ -218,12 +218,10 @@
 }
 
 #[test]
-fn menu_and_stick_clicks_do_nothing_in_either_mode() {
-    let mut c = core();
-    assert_eq!(c.handle(Input::Press(Btn::Menu), &snap()), vec![]);
+fn unused_stick_click_does_nothing_in_either_mode() {
+    let mut c = core();
     assert_eq!(c.handle(Input::Press(Btn::LStick), &snap()), vec![]);
     let mut c = in_respond();
-    assert_eq!(c.handle(Input::Press(Btn::Menu), &snap()), vec![]);
     assert_eq!(c.handle(Input::Press(Btn::LStick), &snap()), vec![]);
 }
 
@@ -514,9 +512,9 @@
 }
 
 #[test]
-fn menu_in_respond_does_not_acknowledge_a_dialog() {
-    let mut c = in_respond();
-    assert_eq!(c.handle(Input::Press(Btn::Menu), &blocked()), vec![]);
+fn unused_stick_in_respond_does_not_acknowledge_a_dialog() {
+    let mut c = in_respond();
+    assert_eq!(c.handle(Input::Press(Btn::LStick), &blocked()), vec![]);
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)]);
 }
 
@@ -626,3 +624,122 @@
     ];
     for (button, label) in cases { assert_eq!(button.label(), label); }
 }
+
+mod closing {
+    use super::*;
+    use crate::herdr::argv_for;
+
+    fn open(c: &mut Core, s: &Snapshot, button: Btn) {
+        assert_eq!(c.handle(Input::Press(button), s), vec![toast("Close actions", "left face: close pane; top face: close space; right face: cancel", Sound::None)]);
+        assert_eq!(c.mode(), Mode::Navigate);
+    }
+    fn selected(c: &mut Core, s: &Snapshot, button: Btn) {
+        let (kind, target) = if button == Btn::X { ("pane", "w5:p1") } else { ("space", "w5") };
+        assert_eq!(c.handle(Input::Press(button), s), vec![toast("Confirm close", &format!("Close {kind} {target}? Stops its running processes. Bottom face: confirm; right face: cancel"), Sound::Request)]);
+    }
+    fn assert_navigation(c: &mut Core, s: &Snapshot) {
+        assert_eq!(c.handle(Input::Press(Btn::X), s), vec![Action::ToggleZoom("w5:p1".into())]);
+    }
+
+    #[test]
+    fn pane_and_space_need_selection_then_confirmation_and_are_consumed_once() {
+        for (button, command) in [(Btn::X, vec!["herdr", "pane", "close", "w5:p1"]), (Btn::Y, vec!["herdr", "workspace", "close", "w5"])] {
+            let s = snap(); let mut c = core();
+            open(&mut c, &s, Btn::Menu);
+            assert_eq!(c.handle(Input::Press(Btn::A), &s), vec![toast("Choose a close action", "left face: pane; top face: space", Sound::None)]);
+            selected(&mut c, &s, button);
+            assert_eq!(c.handle(Input::Press(Btn::A), &s).iter().map(argv_for).collect::<Vec<_>>(), vec![command]);
+            assert_navigation(&mut c, &s);
+        }
+    }
+
+    #[test]
+    fn menu_leaves_respond_and_suppresses_agent_keys_and_navigation() {
+        let s = snap(); let mut c = core();
+        c.handle(Input::Press(Btn::A), &s); assert_eq!(c.mode(), Mode::Respond);
+        open(&mut c, &s, Btn::Menu);
+        for b in [Btn::Up, Btn::Down, Btn::Left, Btn::Right, Btn::Lb, Btn::Rb, Btn::Lt, Btn::Rt, Btn::LStick] {
+            assert_eq!(c.handle(Input::Press(b), &s), vec![]);
+        }
+        selected(&mut c, &s, Btn::X);
+        assert_eq!(c.handle(Input::Release(Btn::A), &s), vec![]);
+        assert_eq!(c.handle(Input::Press(Btn::A), &s).iter().map(argv_for).collect::<Vec<_>>(), vec![vec!["herdr", "pane", "close", "w5:p1"]]);
+    }
+
+    #[test]
+    fn cancel_buttons_clear_both_menu_stages() {
+        for b in [Btn::B, Btn::View, Btn::Menu] { for choose in [false, true] {
+            let s = snap(); let mut c = core(); open(&mut c, &s, Btn::Menu);
+            if choose { selected(&mut c, &s, Btn::Y); }
+            assert_eq!(c.handle(Input::Press(b), &s), vec![toast("Close cancelled", "", Sound::None)]);
+            assert_navigation(&mut c, &s);
+        }}
+    }
+
+    #[test]
+    fn target_changes_or_missing_focus_cancel_instead_of_closing() {
+        for choice in [Btn::X, Btn::Y] { for stage in [false, true] { for kind in 0..3 {
+            let s = snap(); let mut c = core(); open(&mut c, &s, Btn::Menu);
+            if stage { selected(&mut c, &s, choice); }
+            let mut moved = s.clone();
+            match kind { 0 => moved.focused = None, 1 => moved.focused.as_mut().unwrap().pane_id = "w5:p2".into(), _ => moved.focused.as_mut().unwrap().workspace_id = "w9".into() }
+            assert_eq!(c.handle(Input::Press(if stage { Btn::A } else { choice }), &moved), vec![toast("Close cancelled", "focus changed", Sound::None)]);
+            assert_navigation(&mut c, &s);
+        }}}
+    }
+
+    #[test]
+    fn empty_ids_and_no_focus_cannot_open_a_menu() {
+        for kind in 0..3 {
+            let mut s = snap(); let mut c = core();
+            match kind { 0 => s.focused = None, 1 => s.focused.as_mut().unwrap().pane_id.clear(), _ => s.focused.as_mut().unwrap().workspace_id.clear() }
+            assert_eq!(c.handle(Input::Press(Btn::Menu), &s), vec![toast("Nothing to close", "focus a pane first", Sound::None)]);
+            assert_navigation(&mut c, &snap());
+        }
+    }
+
+    #[test]
+    fn voice_and_menu_never_share_a_button() {
+        for voice in [Btn::RStick, Btn::LStick, Btn::Menu] {
+            let s = snap(); let mut c = Core::new(voice);
+            let menu = if voice == Btn::Menu { Btn::LStick } else { Btn::Menu };
+            open(&mut c, &s, menu);
+            assert_eq!(c.handle(Input::Press(voice), &Snapshot::default()), vec![]);
+            assert_eq!(c.handle(Input::Release(voice), &s), vec![]);
+            assert_eq!(c.handle(Input::Press(menu), &s), vec![toast("Close cancelled", "", Sound::None)]);
+            assert_eq!(c.handle(Input::Press(voice), &s), vec![Action::StartRecording]);
+        }
+    }
+
+    #[test]
+    fn voice_in_progress_refuses_menu_without_discarding_recording_or_transcript() {
+        let s = snap(); let mut c = core();
+        assert_eq!(c.handle(Input::Press(Btn::RStick), &s), vec![Action::StartRecording]);
+        assert_eq!(c.handle(Input::Press(Btn::Menu), &s), vec![toast("Voice in progress", "finish dictation before closing", Sound::None)]);
+        assert_eq!(c.handle(Input::Release(Btn::RStick), &s), vec![Action::StopRecording]);
+        assert_eq!(c.handle(Input::Press(Btn::Menu), &s), vec![toast("Voice in progress", "finish dictation before closing", Sound::None)]);
+        assert!(matches!(c.handle(Input::Transcript("hello".into()), &s).first(), Some(Action::Prompt {text, ..}) if text == "hello"));
+        open(&mut c, &s, Btn::Menu);
+    }
+
+    #[test]
+    fn disconnected_controller_discards_close_intent() {
+        let s = snap(); let mut c = core(); open(&mut c, &s, Btn::Menu); selected(&mut c, &s, Btn::Y);
+        assert_eq!(c.handle(Input::Disconnected, &Snapshot::default()), vec![toast("Controller disconnected", "", Sound::None)]);
+        assert_navigation(&mut c, &s);
+    }
+
+    #[test]
+    fn switching_selection_updates_the_explicit_confirmation() {
+        let s = snap(); let mut c = core(); open(&mut c, &s, Btn::Menu);
+        selected(&mut c, &s, Btn::X); selected(&mut c, &s, Btn::Y);
+        assert_eq!(c.handle(Input::Press(Btn::A), &s).iter().map(argv_for).collect::<Vec<_>>(), vec![vec!["herdr", "workspace", "close", "w5"]]);
+    }
+
+    #[test]
+    fn shell_panes_can_be_closed() {
+        let mut s = snap(); s.focused.as_mut().unwrap().agent = None;
+        let mut c = core(); open(&mut c, &s, Btn::Menu); selected(&mut c, &s, Btn::X);
+        assert_eq!(c.handle(Input::Press(Btn::A), &s).iter().map(argv_for).collect::<Vec<_>>(), vec![vec!["herdr", "pane", "close", "w5:p1"]]);
+    }
+}
--- a/src/serve_tests.rs
+++ b/src/serve_tests.rs
@@ -19,13 +19,14 @@
 
 #[test]
 fn without_a_snapshot_only_state_clearing_inputs_reach_the_core() {
-    assert_eq!(on_snapshot_failure(Input::Press(Btn::Up), "no server"), None);
-    assert_eq!(on_snapshot_failure(Input::Press(Btn::A), "no server"), None);
-    let out = on_snapshot_failure(Input::Transcript("x".into()), "no server").unwrap();
+    let mut c = Core::new(Btn::RStick);
+    assert_eq!(on_snapshot_failure(&mut c, Input::Press(Btn::Up), "no server"), None);
+    assert_eq!(on_snapshot_failure(&mut c, Input::Press(Btn::A), "no server"), None);
+    let out = on_snapshot_failure(&mut c, Input::Transcript("x".into()), "no server").unwrap();
     assert!(matches!(&out, Input::TranscriptFailed(e) if e.contains("Herdr unavailable") && e.contains("no server")), "{out:?}");
-    assert_eq!(on_snapshot_failure(Input::Disconnected, "no server"), Some(Input::Disconnected));
-    assert_eq!(on_snapshot_failure(Input::TranscriptFailed("e".into()), "no server"), Some(Input::TranscriptFailed("e".into())));
-    assert_eq!(on_snapshot_failure(Input::Release(Btn::RStick), "no server"), Some(Input::Release(Btn::RStick)));
+    assert_eq!(on_snapshot_failure(&mut c, Input::Disconnected, "no server"), Some(Input::Disconnected));
+    assert_eq!(on_snapshot_failure(&mut c, Input::TranscriptFailed("e".into()), "no server"), Some(Input::TranscriptFailed("e".into())));
+    assert_eq!(on_snapshot_failure(&mut c, Input::Release(Btn::RStick), "no server"), Some(Input::Release(Btn::RStick)));
 }
 
 fn check_transcript_feedback(status: Status) {
@@ -72,3 +73,37 @@
     apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
     assert_eq!(fake.calls().last().map(String::as_str), Some("herdr agent send-keys w5:p1 down"));
 }
+
+#[test]
+fn snapshot_failure_disarms_pending_close() {
+    let s = crate::engine_tests::snap();
+    let mut c = Core::new(Btn::RStick);
+    assert_eq!(c.handle(Input::Press(Btn::Menu), &s), vec![toast("Close actions", "left face: close pane; top face: close space; right face: cancel", Sound::None)]);
+    assert!(matches!(c.handle(Input::Press(Btn::X), &s).first(), Some(Action::Toast { title, .. }) if title == "Confirm close"));
+    on_snapshot_failure(&mut c, Input::Press(Btn::A), "offline");
+    assert_eq!(c.handle(Input::Press(Btn::A), &s), vec![toast("Respond mode", "D-pad moves; bottom confirms, right escapes; Back / Select leaves", Sound::None)]);
+}
+
+#[test]
+fn close_requests_do_not_claim_completion_and_failure_is_reported() {
+    use crate::herdr::Herdr;
+    use crate::herdr_tests::Fake;
+    for choice in [Btn::X, Btn::Y] { for success in [false, true] {
+        let s = crate::engine_tests::snap(); let mut c = Core::new(Btn::RStick);
+        assert_eq!(c.handle(Input::Press(Btn::Menu), &s), vec![toast("Close actions", "left face: close pane; top face: close space; right face: cancel", Sound::None)]);
+        c.handle(Input::Press(choice), &s);
+        let actions = c.handle(Input::Press(Btn::A), &s);
+        let command = if choice == Btn::X { "herdr pane close w5:p1" } else { "herdr workspace close w5" };
+        let fake = Fake::new(vec![(command, if success {Ok("")} else {Err("target gone")}), ("herdr notification show", Ok(""))]);
+        apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
+        let expected = if success { vec![command] } else { vec![command, "herdr notification show Herdr refused --body target gone --sound request"] };
+        assert_eq!(fake.calls(), expected);
+    }}
+}
+
+#[test]
+fn mapping_explains_close_selection_confirmation_and_voice_collision() {
+    for text in ["CLOSE ACTIONS", "Left face button       Select close pane", "Top face button        Select close space", "Bottom face button     Confirm selected close", "Menu / Options opens close actions; if voice uses Menu, use left stick click."] {
+        assert!(MAPPING.contains(text), "mapping is missing {text}");
+    }
+}
--- a/src/engine.rs
+++ b/src/engine.rs
@@ -62,6 +62,8 @@
     FocusDirection { pane_id: String, dir: Dir },
     FocusAgent(String),
     ToggleZoom(String),
+    ClosePane(String),
+    CloseWorkspace(String),
     SendKeys { target: String, keys: Vec<Key> },
     Prompt { target: String, text: String },
     Toast { title: String, body: String, sound: Sound },
@@ -74,7 +76,18 @@
     Action::Toast { title: title.into(), body: body.into(), sound }
 }
 
+#[derive(Clone, Copy)]
+enum CloseKind { Pane, Space }
+
+struct CloseMenu { pane_id: String, workspace_id: String, choice: Option<CloseKind> }
+
+/// The action menu never takes the configured push-to-talk button.
+pub fn menu_button(voice_button: Btn) -> Btn {
+    if voice_button == Btn::Menu { Btn::LStick } else { Btn::Menu }
+}
+
 pub struct Core {
+    close_menu: Option<CloseMenu>,
     mode: Mode,
     voice_button: Btn,
     /// The pane Respond was entered on; a press with another pane focused leaves Respond instead of answering.
@@ -90,13 +103,21 @@
 
 impl Core {
     pub fn new(voice_button: Btn) -> Self {
-        Core { mode: Mode::Navigate, voice_button, respond_pane: None, acknowledged_block: false, recording: false, transcribing: false, voice_target: None }
+        Core { close_menu: None, mode: Mode::Navigate, voice_button, respond_pane: None, acknowledged_block: false, recording: false, transcribing: false, voice_target: None }
     }
     pub fn mode(&self) -> Mode { self.mode }
     #[cfg_attr(not(test), allow(dead_code))]
     pub fn recording(&self) -> bool { self.recording }
 
     pub fn handle(&mut self, input: Input, snap: &Snapshot) -> Vec<Action> {
+        if let Input::Press(b) = input {
+            if self.close_menu.is_some() {
+                // Starting voice needs no snapshot; do not mistake its empty context for moved focus.
+                if b == self.voice_button { return vec![]; }
+                return self.close_press(b, snap);
+            }
+            if b == menu_button(self.voice_button) { return self.open_close(snap); }
+        }
         match input {
             Input::Press(b) if b == self.voice_button => {
                 if self.recording { return vec![]; }
@@ -132,6 +153,51 @@
                 vec![toast("Transcription failed", &e, Sound::Request)]
             }
         }
+    }
+
+    /// A failed snapshot disarms close intent; a later successful read must not revive it.
+    pub fn cancel_close(&mut self) { self.close_menu = None; }
+
+    fn open_close(&mut self, snap: &Snapshot) -> Vec<Action> {
+        if self.recording || self.transcribing {
+            return vec![toast("Voice in progress", "finish dictation before closing", Sound::None)];
+        }
+        let Some(p) = snap.focused.as_ref().filter(|p| !p.pane_id.is_empty() && !p.workspace_id.is_empty()) else {
+            return vec![toast("Nothing to close", "focus a pane first", Sound::None)];
+        };
+        self.enter_navigate();
+        self.close_menu = Some(CloseMenu { pane_id: p.pane_id.clone(), workspace_id: p.workspace_id.clone(), choice: None });
+        vec![toast("Close actions", "left face: close pane; top face: close space; right face: cancel", Sound::None)]
+    }
+
+    fn close_press(&mut self, b: Btn, snap: &Snapshot) -> Vec<Action> {
+        let mut menu = self.close_menu.take().expect("only called with a close menu");
+        if !snap.focused.as_ref().is_some_and(|p| p.pane_id == menu.pane_id && p.workspace_id == menu.workspace_id) {
+            return vec![toast("Close cancelled", "focus changed", Sound::None)];
+        }
+        if b == Btn::B || b == Btn::View || b == menu_button(self.voice_button) {
+            return vec![toast("Close cancelled", "", Sound::None)];
+        }
+        let actions = match b {
+            Btn::X | Btn::Y => {
+                let (kind, target) = if b == Btn::X {
+                    menu.choice = Some(CloseKind::Pane);
+                    ("pane", &menu.pane_id)
+                } else {
+                    menu.choice = Some(CloseKind::Space);
+                    ("space", &menu.workspace_id)
+                };
+                vec![toast("Confirm close", &format!("Close {kind} {target}? Stops its running processes. Bottom face: confirm; right face: cancel"), Sound::Request)]
+            }
+            Btn::A => match menu.choice {
+                Some(CloseKind::Pane) => return vec![Action::ClosePane(menu.pane_id)],
+                Some(CloseKind::Space) => return vec![Action::CloseWorkspace(menu.workspace_id)],
+                None => vec![toast("Choose a close action", "left face: pane; top face: space", Sound::None)],
+            },
+            _ => vec![],
+        };
+        self.close_menu = Some(menu);
+        actions
     }
 
     fn press(&mut self, b: Btn, snap: &Snapshot) -> Vec<Action> {
@@ -185,6 +251,7 @@
     }
 
     fn enter_navigate(&mut self) {
+        self.cancel_close();
         self.mode = Mode::Navigate;
         self.respond_pane = None;
         self.acknowledged_block = false;
--- a/src/herdr.rs
+++ b/src/herdr.rs
@@ -198,6 +198,8 @@
 pub fn argv_for(a: &Action) -> Vec<String> {
     let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
     match a {
+        Action::ClosePane(id) => s(&["herdr", "pane", "close", id]),
+        Action::CloseWorkspace(id) => s(&["herdr", "workspace", "close", id]),
         Action::FocusWorkspace(id) => s(&["herdr", "workspace", "focus", id]),
         Action::FocusTab(id) => s(&["herdr", "tab", "focus", id]),
         Action::FocusDirection { pane_id, dir } => s(&["herdr", "pane", "focus", "--direction", dir_name(*dir), "--pane", pane_id]),
--- a/src/serve.rs
+++ b/src/serve.rs
@@ -42,7 +42,7 @@
                 Some(Err(e)) => {
                     crate::logging::event(format_args!("herd-gamepad: snapshot: {e}"));
                     let _ = herdr.apply(&toast("Herdr unavailable", &e, Sound::Request));
-                    match on_snapshot_failure(input, &e) {
+                    match on_snapshot_failure(&mut core, input, &e) {
                         Some(i) => input = i,
                         None => continue,
                     }
@@ -107,7 +107,8 @@
 /// clear state go through with an empty snapshot (the recording must still stop, and then its transcript is
 /// dropped as "nothing was focused"); a transcript that cannot be routed fails instead, so the next recording
 /// can start.
-pub fn on_snapshot_failure(input: Input, err: &str) -> Option<Input> {
+pub fn on_snapshot_failure(core: &mut Core, input: Input, err: &str) -> Option<Input> {
+    core.cancel_close();
     match input {
         Input::Transcript(_) => Some(Input::TranscriptFailed(format!("Herdr unavailable: {err}"))),
         Input::Release(_) | Input::Disconnected | Input::TranscriptFailed(_) => Some(input),
@@ -234,6 +235,15 @@
 Back / Select          Return to Navigate
 Shoulders, triggers,
   top or left face     Navigate as above and leave Respond
+
+CLOSE ACTIONS
+Menu / Options opens close actions; if voice uses Menu, use left stick click.
+Left face button       Select close pane
+Top face button        Select close space
+Bottom face button     Confirm selected close
+Right face / Back      Cancel; pressing the menu button again also cancels
+Closing stops running processes in the pane or space. Focus changes cancel.
+Finish dictation before opening close actions.
 
 A D-pad press on a blocked agent auto-switches to Respond.
 Each confirmation needs acknowledgement first; follow the on-screen prompt.
--- a/README.md
+++ b/README.md
@@ -69,6 +69,7 @@
 | Left / right trigger | LT / RT | L2 / R2 | Previous / next tab |
 | Back / Select | View | Create (PS5) / Share (PS4) | Return to Navigate |
 | Right stick click | Right stick click | R3 | Hold to dictate (default) |
+| Menu / Options | Menu | Options | Open close actions |
 
 D-pad moves between panes in Navigate, and between options in Respond. On a blocked
 agent, D-pad automatically enters Respond. Each confirmation requires acknowledgement
@@ -79,6 +80,16 @@
 working agent—no extra Enter or controller press is needed. If focus changed, the pane
 has no agent, or the agent is blocked at a dialog or in an unknown state, the transcript
 is not sent.
+
+To close a pane or space, press **Menu / Options** to show close actions. Press the
+**left face button (X / Square)** to select Close pane, or the **top face button
+(Y / Triangle)** to select Close space. The confirmation names the target; press the
+**bottom face button (A / Cross)** to confirm. **Right face (B / Circle)**,
+**Back / Select**, or the menu button cancels. Closing stops running processes in that
+pane or space; closing a space includes all its panes and tabs, not its workspace group.
+A focus change or controller disconnect cancels the pending action. Finish dictation
+before opening close actions. If `[voice] button = "menu"`, use **left stick click
+(L3)** to open/cancel close actions instead.
 
 To inspect a controller's events, run `herd-gamepad probe`; Ctrl-C stops it.
 
--- a/CHANGELOG.md
+++ b/CHANGELOG.md
@@ -1,6 +1,8 @@
 # Changelog
 
 ## Unreleased
+
+- Add controller close-pane and close-space actions with explicit target confirmation and cancellation.
 
 - Submit dictation automatically to working agents as well as idle/done agents; no extra Enter press.
 
```
