# Single-press waiting-agent confirmation implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Let one physical Cross press answer a waiting agent from Navigate or Respond.

**Architecture:** Entering Respond on a focused blocked agent emits its normal mode feedback plus exactly one agent Enter. Other agent/non-agent entries still consume the entry press. Respond maps every Cross to Enter after the existing pane-focus and agent-presence guards. Remove obsolete acknowledgement state and parameters.

**Tech Stack:** Existing Rust state machine, Herdr adapter, cargo test/mutants.
**Baseline:** `8ca50e50451329c00c7e756b0e7445c861bc51f2`.

## Intent and scope

User explicitly requested removing the extra confirmation press before answering an agent question. Enter applies the agent's current selection; this change does not force a Yes answer or infer dialogue content. Repeated physical Cross presses each send one Enter. Entry on idle/working/done/unknown agents and non-agent panes does not send Enter. Focus changes during Respond still consume the triggering press and exit; a later deliberate Cross can answer the newly focused blocked agent.

Keep Circle exit, Share Escape, Navigate D-pad focus, Interact pane targeting, dictation's blocked-agent refusal, and close confirmation unchanged. Version remains 0.2.0; add its changelog bullet. No release publication in this task.

## Task 1: tests first

**Files:** `src/engine_tests.rs`, `src/serve_tests.rs`.

Apply the following exact patch and run `cargo test --locked --quiet`. The updated/new assertions must fail against baseline. Old acknowledgement tests are renamed and rewritten to assert direct delivery across repeated/new dialogs; the existing focus, agent-presence, disconnect, dictation and close-menu tests remain. The explicit entry-status test pairs its negative nonblocked/non-agent cases with a currently-failing positive blocked-entry assertion.

Commit with `test: specify single-press waiting-agent confirmation`.

```diff
--- a/src/engine_tests.rs
+++ b/src/engine_tests.rs
@@ -226,18 +226,18 @@
     let mut c = core();
     assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Down }]);
     assert_eq!(c.mode(), Mode::Navigate);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)]);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
-}
-
-#[test]
-fn a_on_a_blocked_agent_only_enters_respond_mode() {
-    // An approval is always two deliberate presses: A to enter Respond, A again to confirm.
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), blocked_entry("w5:p1"));
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
+}
+
+#[test]
+fn a_on_a_blocked_agent_enters_respond_and_sends_enter() {
+    // One deliberate Cross both enters Respond and confirms the waiting agent.
     let mut c = core();
     let out = c.handle(Input::Press(Btn::A), &blocked());
     assert_eq!(c.mode(), Mode::Respond);
-    assert_eq!(out, vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)]);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter), "the second A confirms");
+    assert_eq!(out, blocked_entry("w5:p1"));
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter), "another deliberate Cross sends another Enter");
 }
 
 #[test]
@@ -382,7 +382,7 @@
     let s = Snapshot { focused: Some(pane("w5:p2", Some("claude"), Some(Status::Blocked))), ..snap() };
     assert_eq!(c.handle(Input::Press(Btn::A), &s), focus_moved());
     assert_eq!(c.mode(), Mode::Navigate);
-    assert_eq!(c.handle(Input::Press(Btn::A), &s), vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)], "A in Navigate enters Respond on the new pane without sending Enter");
+    assert_eq!(c.handle(Input::Press(Btn::A), &s), blocked_entry("w5:p2"), "the next deliberate press answers the newly focused blocked agent");
 }
 
 #[test]
@@ -406,70 +406,69 @@
 }
 
 #[test]
-fn each_dialog_needs_its_own_acknowledgement() {
-    let mut c = in_respond();
-    // Dialog 1: the D-pad press acknowledges it, A answers it.
+fn each_dialog_accepts_one_cross_press() {
+    let mut c = in_respond();
+    // Arrow keys and Cross are delivered directly in Respond.
     assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), keys("w5:p1", Key::Down));
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
     // The agent works again; keys still go through.
     assert_eq!(c.handle(Input::Press(Btn::Down), &working()), keys("w5:p1", Key::Down));
-    // Dialog 2: the first A is swallowed, the second confirms.
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
+    // A newly blocked agent accepts the next Cross immediately.
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
     assert_eq!(c.mode(), Mode::Respond);
 }
 
 #[test]
-fn a_on_a_dialog_that_appeared_after_entering_respond_is_swallowed_once() {
-    let mut c = in_respond();
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
-}
-
-#[test]
-fn share_on_a_fresh_dialog_is_sent_and_counts_as_the_acknowledgement() {
+fn a_on_a_new_dialog_sends_enter_immediately() {
+    let mut c = in_respond();
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
+}
+
+#[test]
+fn share_escape_does_not_delay_the_next_enter() {
     let mut c = in_respond();
     assert_eq!(c.handle(Input::Press(Btn::View), &blocked()), keys("w5:p1", Key::Esc));
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
 }
 
 #[test]
-fn consecutive_a_presses_on_an_acknowledged_dialog_alternate_between_enter_and_a_swallow() {
-    // The core cannot see status changes between presses, so a second dialog that follows the first in a quiet
-    // gap looks like the first; every Enter therefore needs its own acknowledging press.
+fn consecutive_cross_presses_each_send_exactly_one_enter() {
+    // Each physical Cross press produces one Enter, even if status stays blocked.
     let mut c = in_respond();
     assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), keys("w5:p1", Key::Down));
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
-}
-
-#[test]
-fn an_enter_after_a_dpad_acknowledgement_spends_it() {
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
+}
+
+#[test]
+fn an_enter_after_dpad_does_not_delay_the_next_enter() {
     let mut c = in_respond();
     c.handle(Input::Press(Btn::Down), &blocked());
     c.handle(Input::Press(Btn::A), &blocked());
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)], "nothing was pressed since the Enter");
-}
-
-#[test]
-fn an_enter_after_entering_on_a_blocked_pane_spends_the_acknowledgement() {
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter), "every Cross sends Enter");
+}
+
+#[test]
+fn every_enter_after_blocked_entry_is_sent() {
     let mut c = core();
     c.handle(Input::Press(Btn::A), &blocked());
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
-}
-
-#[test]
-fn disconnecting_leaves_respond_and_forgets_the_acknowledgement() {
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
+}
+
+#[test]
+fn disconnecting_leaves_respond_and_allows_single_press_reentry() {
     let mut c = in_respond();
     assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), keys("w5:p1", Key::Down));
     assert_eq!(c.handle(Input::Disconnected, &blocked()), vec![toast("Controller disconnected", "", Sound::None)]);
     assert_eq!(c.mode(), Mode::Navigate);
     assert_eq!(c.handle(Input::Connected, &blocked()), vec![toast("Controller connected", "", Sound::None)]);
     assert_eq!(c.mode(), Mode::Navigate);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), blocked_entry("w5:p1"));
     assert_eq!(c.mode(), Mode::Respond);
 }
 
@@ -485,24 +484,24 @@
 }
 
 #[test]
-fn leaving_respond_forgets_the_acknowledgement() {
+fn leaving_respond_does_not_delay_a_later_confirmation() {
     let mut c = in_respond();
     c.handle(Input::Press(Btn::Down), &blocked());
     c.handle(Input::Press(Btn::B), &blocked());
-    // Re-entering with A on the blocked pane: that A is the acknowledgement, the next confirms.
+    // Re-entering on a blocked pane sends Enter immediately.
     c.handle(Input::Press(Btn::A), &blocked());
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
-    // Re-entering with A on an idle pane that then blocks: the first A is swallowed.
+    // Entry on idle is consumed; a later blocked confirmation is immediate.
     c.handle(Input::Press(Btn::B), &snap());
     c.handle(Input::Press(Btn::A), &snap());
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
-}
-
-#[test]
-fn unused_stick_in_respond_does_not_acknowledge_a_dialog() {
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
+}
+
+#[test]
+fn unused_stick_in_respond_does_not_affect_confirmation() {
     let mut c = in_respond();
     assert_eq!(c.handle(Input::Press(Btn::LStick), &blocked()), vec![]);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
 }
 
 #[test]
@@ -806,7 +805,9 @@
         assert_eq!(c.mode(),Mode::Navigate);
         assert_eq!(c.handle(Input::Press(Btn::B),&s),vec![]);
         let entry=c.handle(Input::Press(Btn::A),&s);
-        assert!(matches!(entry.as_slice(),[Action::Toast{..}]));
+        if s.focused.as_ref().is_some_and(|p| p.status == Some(Status::Blocked)) {
+            assert_eq!(entry,blocked_entry("w5:p1"));
+        } else { assert!(matches!(entry.as_slice(),[Action::Toast{..}])); }
         assert_ne!(c.mode(),Mode::Navigate);
     }
 }
@@ -827,7 +828,7 @@
     assert_eq!(c.handle(Input::Press(Btn::View),&blocked()),keys("w5:p1",Key::Esc));
     assert_eq!(c.mode(),Mode::Respond);
     assert_eq!(c.handle(Input::Press(Btn::A),&blocked()),keys("w5:p1",Key::Enter));
-    assert!(matches!(c.handle(Input::Press(Btn::A),&blocked()).as_slice(),[Action::Toast{..}]));
+    assert_eq!(c.handle(Input::Press(Btn::A),&blocked()),keys("w5:p1",Key::Enter));
     let mut c=in_interact();
     assert_eq!(c.handle(Input::Press(Btn::View),&non_agent()),vec![Action::SendPaneKeys{target:"w5:p2".into(),keys:vec![Key::Esc]}]);
     assert_eq!(c.mode(),Mode::Interact);
@@ -845,3 +846,22 @@
     assert_eq!(c.handle(Input::Press(Btn::Down),&blocked()),vec![Action::FocusDirection{pane_id:"w5:p1".into(),dir:Dir::Down}]);
     assert_eq!(c.mode(),Mode::Navigate);
 }
+
+fn blocked_entry(target: &str) -> Vec<Action> {
+    vec![toast("Respond mode", "D-pad moves; Cross confirms; Circle returns to Navigate; Create / Share escapes", Sound::None),
+        Action::SendKeys{target:target.into(),keys:vec![Key::Enter]}]
+}
+#[test]
+fn single_press_is_only_a_shortcut_for_blocked_agent_entry() {
+    assert_eq!(core().handle(Input::Press(Btn::A),&blocked()),blocked_entry("w5:p1"));
+    for status in [Status::Idle,Status::Working,Status::Done,Status::Unknown] {
+        let mut s=snap();s.focused.as_mut().unwrap().status=Some(status);
+        let mut c=core();let out=c.handle(Input::Press(Btn::A),&s);
+        assert_eq!(c.mode(),Mode::Respond);
+        assert!(matches!(out.as_slice(),[Action::Toast{..}]));
+    }
+    let mut s=snap();s.focused.as_mut().unwrap().status=None;
+    assert!(matches!(core().handle(Input::Press(Btn::A),&s).as_slice(),[Action::Toast{..}]));
+    assert!(matches!(core().handle(Input::Press(Btn::A),&non_agent()).as_slice(),[Action::Toast{..}]));
+    assert_eq!(said("yes",&blocked()),vec![toast("Not sent","agent is waiting at a dialog: yes",Sound::Request)]);
+}
--- a/src/serve_tests.rs
+++ b/src/serve_tests.rs
@@ -72,7 +72,7 @@
         ("herdr agent send-keys", Ok("")),
     ]);
     apply_actions(&Herdr { runner: &fake }, actions, &mut crate::feedback::Feedback::default(), |_| panic!("unexpected recording"));
-    assert_eq!(fake.calls(), vec!["herdr agent send-keys w5:p1 down"]);
+    assert_eq!(fake.calls(), vec!["herdr agent send-keys w5:p1 enter", "herdr agent send-keys w5:p1 down"]);
 }
 
 #[test]
@@ -212,3 +212,9 @@
     assert_eq!(MAPPING.matches("Circle (Xbox: B)       Return to Navigate").count(),2);
     assert_eq!(MAPPING.matches("Create / Share (View)  Escape / back in the app").count(),2);
 }
+
+#[test]
+fn mapping_explains_single_press_waiting_agent_confirmation() {
+    assert!(MAPPING.contains("One Cross (Xbox: A) press answers a waiting agent, including from Navigate."));
+    assert!(!MAPPING.contains("acknowledgement"));
+}
```

## Task 2: remove the acknowledgement gate

**Files:** `src/engine.rs`, `src/serve.rs`, `README.md`, `CHANGELOG.md`.

Apply the patch below. Run `cargo test --locked --quiet` and `cargo build --locked`; expect 168 tests and build to pass. Run `cargo mutants --file src/engine.rs --jobs 4 --output /tmp/single-press-mutation-check`. Change the new blocked shortcut condition to always true and always false separately: tests must fail in both cases. Remove each retained Respond focus/agent guard individually to verify those protections still have effective tests; restore source afterward.

Commit with `feat: answer waiting agents with one controller press`.

```diff
--- a/src/engine.rs
+++ b/src/engine.rs
@@ -95,8 +95,6 @@
     respond_pane: Option<String>,
     /// Explicitly selected non-agent pane; an agent appearing invalidates this mode.
     interact_pane: Option<String>,
-    /// The user has pressed a key while this pane's agent was Blocked: the next A may answer the dialog.
-    acknowledged_block: bool,
     recording: bool,
     /// A transcription is running; its result decides `voice_target`'s fate.
     transcribing: bool,
@@ -106,7 +104,7 @@
 
 impl Core {
     pub fn new(voice_button: Btn) -> Self {
-        Core { close_menu: None, mode: Mode::Navigate, voice_button, respond_pane: None, interact_pane: None, acknowledged_block: false, recording: false, transcribing: false, voice_target: None }
+        Core { close_menu: None, mode: Mode::Navigate, voice_button, respond_pane: None, interact_pane: None, recording: false, transcribing: false, voice_target: None }
     }
     pub fn mode(&self) -> Mode { self.mode }
     #[cfg_attr(not(test), allow(dead_code))]
@@ -139,7 +137,7 @@
             Input::Release(_) => vec![],
             Input::Connected => vec![toast("Controller connected", "", Sound::None)],
             Input::Disconnected => {
-                // Whatever was acknowledged on the old controller does not carry over to the next one.
+                // A disconnected controller leaves every interaction mode.
                 self.enter_navigate();
                 if !self.recording { return vec![toast("Controller disconnected", "", Sound::None)]; }
                 self.recording = false;
@@ -243,11 +241,13 @@
                     .map(|p| vec![Action::FocusDirection { pane_id: p.pane_id.clone(), dir: dir_of(b) }])
                     .unwrap_or_default(),
                 Btn::A => match agent_pane {
-                    Some(_) => {
-                        // Entering on a blocked agent is the acknowledgement; the next A confirms.
-                        self.enter_respond(agent_pane, blocked);
-                        if blocked { vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)] }
-                        else { vec![toast("Respond mode", "D-pad moves; Cross confirms; Circle returns to Navigate; Create / Share escapes", Sound::None)] }
+                    Some(p) => {
+                        self.enter_respond(p);
+                        let mut out = vec![toast("Respond mode", "D-pad moves; Cross confirms; Circle returns to Navigate; Create / Share escapes", Sound::None)];
+                        if blocked {
+                            out.push(Action::SendKeys { target: p.pane_id.clone(), keys: vec![Key::Enter] });
+                        }
+                        out
                     }
                     None => match focused.filter(|p| !p.pane_id.is_empty()) {
                         Some(p) => {
@@ -275,10 +275,9 @@
         }
     }
 
-    fn enter_respond(&mut self, agent_pane: Option<&Pane>, acknowledged: bool) {
+    fn enter_respond(&mut self, pane: &Pane) {
         self.mode = Mode::Respond;
-        self.respond_pane = agent_pane.map(|p| p.pane_id.clone());
-        self.acknowledged_block = acknowledged;
+        self.respond_pane = Some(pane.pane_id.clone());
     }
 
     fn enter_navigate(&mut self) {
@@ -286,7 +285,6 @@
         self.mode = Mode::Navigate;
         self.respond_pane = None;
         self.interact_pane = None;
-        self.acknowledged_block = false;
     }
 
     fn leave_respond(&mut self, actions: Vec<Action>) -> Vec<Action> {
@@ -314,16 +312,6 @@
             return vec![toast("Navigate mode", "no agent in the focused pane", Sound::None)];
         };
         let Some(key) = key_for(b) else { return vec![] };
-        // Every Enter needs its own acknowledgement: a key pressed while the agent is Blocked (an arrow, Esc, or
-        // a swallowed A) acknowledges the dialog, and the Enter that answers it spends the acknowledgement. The
-        // core cannot see the status change between two presses, so a second dialog that follows the first in a
-        // quiet gap must not be confirmed by the acknowledgement of the first.
-        let blocked_now = p.status == Some(Status::Blocked);
-        if blocked_now && !self.acknowledged_block && key == Key::Enter {
-            self.acknowledged_block = true;
-            return vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)];
-        }
-        self.acknowledged_block = blocked_now && key != Key::Enter;
         vec![Action::SendKeys { target: p.pane_id.clone(), keys: vec![key] }]
     }
 }
--- a/src/serve.rs
+++ b/src/serve.rs
@@ -289,7 +289,7 @@
 Finish dictation before opening close actions.
 
 D-pad always moves pane focus in Navigate; Cross enters Respond on an agent.
-Each blocked-agent confirmation needs acknowledgement first; press Cross (Xbox: A) again.
+One Cross (Xbox: A) press answers a waiting agent, including from Navigate.
 Routine status updates are in Gamepad logs. Errors are shown at most once per 5 seconds.
 Close actions use a live popup; wait for the selected target to appear before confirming.
 Button positions define the actions, even when your controller uses other labels.
--- a/README.md
+++ b/README.md
@@ -61,7 +61,7 @@
 
 | Physical control | Xbox | PlayStation | Action |
 |---|---|---|---|
-| Bottom face | A | Cross | Enter Respond / Interact; Enter when inside |
+| Bottom face | A | Cross | Enter Respond / Interact; answer a waiting agent |
 | Right face | B | Circle | Return to Navigate |
 | Left face | X | Square | Toggle zoom; Space in Interact |
 | Top face | Y | Triangle | Next agent needing attention |
@@ -72,8 +72,9 @@
 | Menu / Options | Menu | Options | Open close actions |
 
 D-pad moves between panes in Navigate, even when an agent is blocked. Press Cross
-(Xbox: A) to enter Respond and move between its options. Each confirmation requires acknowledgement
-first; press Cross (Xbox: A) again to confirm. Face-button positions always determine actions,
+(Xbox: A) to enter Respond and move between its options. If the agent is waiting at
+a question, that first Cross press also sends Enter. In Respond, every Cross press
+sends Enter immediately; no extra confirmation press is required. Face-button positions always determine actions,
 even on controllers with different lettering.
 
 For a non-agent pane such as PR Tracker, press **Cross (Xbox: A)** to enter **Interact**.
--- a/CHANGELOG.md
+++ b/CHANGELOG.md
@@ -2,25 +2,21 @@
 
 ## 0.2.0
 
+- Answer a waiting agent with one Cross press from Navigate or Respond.
 - Interact with non-agent panes such as PR Tracker using arrows, Enter, Space, and Escape.
 - Use Cross to enter a pane, Circle to return to navigation, and Share/Create for Escape.
 - Keep D-pad pane navigation available even when an agent is waiting for input.
 - Show familiar PlayStation button names with explicit Xbox equivalents in close controls.
 - Keep close-menu feedback current and bound error notifications to avoid stale popup queues.
-
 - Deliver completed dictation without waiting for another controller button press.
-
 - Add controller close-pane and close-space actions with explicit target confirmation and cancellation.
-
 - Submit dictation automatically to working agents as well as idle/done agents; no extra Enter press.
-
 - Run Gamepad as a Herdr background action with explicit Start and Stop controls.
 - Cancel active voice work and wait for shutdown on Stop.
 - Add on-demand recent logs in an overlay, with bounded log rotation.
 - Keep the mapping overlay on demand; starting no longer opens a daemon tab.
 - Upgrade from v0.1.0 by closing its old Gamepad pane once before starting.
 - Publish stable version tags automatically after both macOS CI jobs pass, with version and tag validation.
-
 
 ## 0.1.0
 
```

## Task 3: review and validate

Use fresh implementation, spec review, quality review and final different-model review before release. On hardware, use D-pad to select the desired option in Respond, then one Cross to answer it. Also test one Cross from Navigate on a waiting agent, Circle back to navigation, and continued D-pad pane focus. Do not send live approvals during automated checks.

## Verification evidence

Scratch: `/tmp/gamepad-single-scratch`; no production edits or commits.

- `/tmp/single-red.log`: 16 changed/new assertions fail before implementation.
- `/tmp/single-green.log`: 164 unit + 4 integration tests pass.
- `/tmp/single-build.log`: build passes. Final patches recreate scratch byte-for-byte in `/tmp/single-plan-assembled`; `/tmp/single-assembled-red.log` shows 16 failing assertions against baseline, and `/tmp/single-assembled-green.log` plus `/tmp/single-assembled-build.log` show all 168 tests/build passing. Final changelog spacing cleanup changes documentation only. Both patches pass git apply --check on the feature worktree.
- `/tmp/single-guard-{blocked-only,blocked-shortcut,respond-focus,respond-agent}.log`: both shortcut-condition mutations and both retained safety-guard removals fail assertions.
- `/tmp/single-mutants.log` and `/tmp/single-mutation/mutants.out/outcomes.json`: 123 engine mutants tested, 108 caught, 15 unviable, zero survivors or timeouts.
