# Non-agent pane interaction implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Let a controller navigate terminal applications such as PR Tracker through an explicitly entered Interact mode.

**Architecture:** Keep Navigate and agent Respond behavior intact. Add Interact bound to one non-agent pane and a distinct SendPaneKeys action using Herdr's pane command. Its first Cross press enters without sending a terminal key; subsequent inputs implement the agreed keyboard mapping.

**Tech Stack:** Rust state machine, existing Herdr CLI adapter, cargo test, cargo mutants.

**Design:** `docs/plans/2026-10-27-pane-interaction-design.md`.
**Baseline:** `c8d88459bde92f5ddce78755752ffe135f0f1dc9` plus the design-only commit.

## Verified facts and scope

The isolated receiver captured `herdr pane send-keys` arrows, Enter, Space and Escape as expected. PR Tracker's actual `src/ui.rs` accepts these keys. No PR Tracker-specific command or detection is added. This covers ordinary terminals, including shells. Voice and close-menu handling retain their existing priority. Focus/agent changes are detected in the per-button snapshot; this does not claim atomic validation of changes occurring after a snapshot. Do not modify PR5, installation, or release behavior.

## Task 1: regression tests

**Files:** `src/engine_tests.rs`, `src/herdr_tests.rs`, `src/serve_tests.rs`.

1. Apply the following test patch. It replaces the now-obsolete no-agent rejection test. The debug assertions intentionally compile against the baseline before new enum variants exist, so RED is a behavioral assertion failure rather than a compilation failure. Every negative safety case first enters Interact through a positive assertion.
2. Run `cargo test --locked interact_` and `cargo test --locked mapping_`. Expect six feature test failures, a new mapping content failure, and the expanded row-count failure. Record the assertions, not merely the nonzero status.
3. Commit with `test: specify controller interaction with non-agent panes`.

```diff
--- a/src/engine_tests.rs
+++ b/src/engine_tests.rs
@@ -138,14 +138,6 @@
     assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "Respond mode"), "{out:?}");
 }
 
-#[test]
-fn a_in_navigate_toasts_when_the_pane_has_no_agent() {
-    let mut c = core();
-    let s = Snapshot { focused: Some(pane("w5:p2", None, None)), ..snap() };
-    let out = c.handle(Input::Press(Btn::A), &s);
-    assert_eq!(c.mode(), Mode::Navigate);
-    assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "No agent here"), "{out:?}");
-}
 
 fn in_respond() -> Core {
     let mut c = core();
@@ -743,3 +735,69 @@
         assert_eq!(c.handle(Input::Press(Btn::A), &s).iter().map(argv_for).collect::<Vec<_>>(), vec![vec!["herdr", "pane", "close", "w5:p1"]]);
     }
 }
+
+fn non_agent() -> Snapshot {
+    Snapshot { focused: Some(pane("w5:p2", None, None)), ..snap() }
+}
+fn in_interact() -> Core {
+    let mut c = core();
+    let out = c.handle(Input::Press(Btn::A), &non_agent());
+    assert_eq!(format!("{:?}", c.mode()), "Interact");
+    assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "Interact mode"));
+    c
+}
+#[test]
+fn interact_enters_without_sending_and_maps_all_keys() {
+    let mut c = in_interact();
+    for (button, key) in [(Btn::Up,"Up"),(Btn::Down,"Down"),(Btn::Left,"Left"),(Btn::Right,"Right"),(Btn::A,"Enter"),(Btn::B,"Esc"),(Btn::X,"Space")] {
+        assert_eq!(format!("{:?}", c.handle(Input::Press(button), &non_agent())), format!("[SendPaneKeys {{ target: \"w5:p2\", keys: [{key}] }}]"));
+        assert_eq!(format!("{:?}", c.mode()), "Interact");
+    }
+    assert_eq!(c.handle(Input::Press(Btn::LStick), &non_agent()), vec![]);
+}
+#[test]
+fn interact_rejects_missing_or_empty_target() {
+    let _ = in_interact(); // Positive assertion fails before the feature exists.
+    for focused in [None, Some(Pane::default())] {
+        let mut c=core(); let s=Snapshot{focused,..non_agent()};
+        let out=c.handle(Input::Press(Btn::A), &s);
+        assert_eq!(c.mode(),Mode::Navigate);
+        assert!(matches!(out.as_slice(),[Action::Toast{title,..}] if title=="No pane here"));
+    }
+}
+#[test]
+fn interact_focus_or_agent_change_consumes_each_press() {
+    let mut missing=non_agent(); missing.focused=None;
+    let mut moved=non_agent(); moved.focused.as_mut().unwrap().pane_id="w5:p3".into();
+    let mut agent=non_agent(); agent.focused.as_mut().unwrap().agent=Some("codex".into());
+    for s in [missing,moved,agent] {
+        for b in [Btn::A,Btn::B,Btn::X,Btn::Y,Btn::Up,Btn::Down,Btn::Left,Btn::Right,Btn::Lb,Btn::Rb,Btn::Lt,Btn::Rt] {
+            let mut c=in_interact();
+            let out=c.handle(Input::Press(b), &s);
+            assert_eq!(c.mode(),Mode::Navigate);
+            assert!(matches!(out.as_slice(),[Action::Toast{title,..}] if title=="Navigate mode"));
+            assert_eq!(c.handle(Input::Press(Btn::X), &non_agent()), vec![Action::ToggleZoom("w5:p2".into())]);
+        }
+    }
+}
+#[test]
+fn interact_view_disconnect_and_global_navigation_exit() {
+    for b in [Btn::View,Btn::Lb,Btn::Rb,Btn::Lt,Btn::Rt,Btn::Y] {
+        let mut c=in_interact();
+        let expected=core().handle(Input::Press(b),&non_agent());
+        let out=c.handle(Input::Press(b),&non_agent());
+        assert_eq!(c.mode(),Mode::Navigate);
+        assert_eq!(&out[1..],expected.as_slice());
+    }
+    let mut c=in_interact(); c.handle(Input::Disconnected,&non_agent());
+    assert_eq!(c.mode(),Mode::Navigate);
+    assert_eq!(c.handle(Input::Press(Btn::Up),&non_agent()),vec![Action::FocusDirection{pane_id:"w5:p2".into(),dir:Dir::Up}]);
+}
+#[test]
+fn interact_close_menu_still_owns_its_buttons() {
+    let mut c=in_interact(); c.handle(Input::Press(Btn::Menu),&non_agent());
+    assert_eq!(c.mode(),Mode::Navigate);
+    assert!(c.close_view().is_some());
+    c.handle(Input::Press(Btn::X),&non_agent());
+    assert_eq!(c.handle(Input::Press(Btn::A),&non_agent()),vec![Action::ClosePane("w5:p2".into())]);
+}
--- a/src/herdr_tests.rs
+++ b/src/herdr_tests.rs
@@ -293,3 +293,15 @@
     let result = run_with_cancellation(&["/nonexistent/never-spawn"], Duration::from_secs(10), || true);
     assert_eq!(result, Err("command cancelled".into()));
 }
+
+#[test]
+fn interact_routes_non_agent_keys_through_pane_interface() {
+    let mut c=Core::new(Btn::RStick);
+    let s=Snapshot { focused: Some(Pane{pane_id:"w5:p2".into(),..Pane::default()}),..Snapshot::default() };
+    c.handle(Input::Press(Btn::A),&s);
+    for (b,key) in [(Btn::Up,"up"),(Btn::Down,"down"),(Btn::Left,"left"),(Btn::Right,"right"),(Btn::A,"enter"),(Btn::B,"esc"),(Btn::X,"space")] {
+        let actions=c.handle(Input::Press(b),&s);
+        assert_eq!(actions.len(),1);
+        assert_eq!(argv_for(&actions[0]),vec!["herdr","pane","send-keys","w5:p2",key]);
+    }
+}
--- a/src/serve_tests.rs
+++ b/src/serve_tests.rs
@@ -190,10 +190,17 @@
 fn mapping_button_descriptions_align_with_navigation_rows() {
     let labels = ["Triangle (Xbox: Y)", "Square (Xbox: X)", "Cross (Xbox: A)", "Circle (Xbox: B)", "Create / Share (View)"];
     let rows: Vec<_> = MAPPING.lines().filter(|line| labels.iter().any(|label| line.starts_with(label))).collect();
-    assert_eq!(rows.len(), 10);
+    assert_eq!(rows.len(), 14);
     let expected = MAPPING.lines().find(|line| line.starts_with("D-pad ")).unwrap().find("Focus").unwrap();
     for row in rows {
         let after_label = &row[row.find(')').unwrap() + 1..];
         assert_eq!(row.len() - after_label.trim_start().len(), expected, "misaligned mapping row: {row}");
     }
 }
+
+#[test]
+fn mapping_explains_non_agent_interact_keys_and_escape() {
+    for text in ["INTERACT (NON-AGENT PANES)", "Enter Respond / Interact", "Send arrow keys to the pane", "Space / select", "Return to Navigate"] {
+        assert!(MAPPING.contains(text), "mapping is missing {text}");
+    }
+}
```

## Task 2: implementation and controls documentation

**Files:** `src/engine.rs`, `src/herdr.rs`, `src/serve.rs`, `README.md`.

1. Apply the exact patch below. The focus/agent check must precede global navigation dispatch, and Square's zoom mapping must exclude Interact. Do not reroute agent SendKeys through the pane interface.
2. Run `cargo test --locked --quiet` and `cargo build --locked`. Expect all 161 tests and the build to pass.
3. Run `cargo mutants --file src/engine.rs --file src/herdr.rs --jobs 4 --output /tmp/pane-interaction-mutants`. Evaluate survivors in changed pure logic with failing assertions before changing code. Existing I/O-helper survivors listed below are outside this change.
4. Delete each new guard in turn and run `cargo test --locked interact_`, restoring the original after every experiment: whole Interact validation; pane-ID comparison; agent-absence comparison; nonempty entry target; Space-key selection; Square's Interact exclusion; View exit condition; Navigate-only early return in leave_respond. Each must fail an assertion.
5. Commit with `feat: navigate non-agent panes with controller interact mode`.

```diff
--- a/src/engine.rs
+++ b/src/engine.rs
@@ -24,7 +24,7 @@
 pub enum Input { Press(Btn), Release(Btn), Connected, Disconnected, Transcript(String), TranscriptFailed(String) }
 
 #[derive(Clone, Copy, Debug, PartialEq, Eq)]
-pub enum Mode { Navigate, Respond }
+pub enum Mode { Navigate, Respond, Interact }
 
 #[derive(Clone, Copy, Debug, PartialEq, Eq)]
 pub enum Status { Idle, Working, Blocked, Done, Unknown }
@@ -49,7 +49,7 @@
 pub enum Dir { Left, Right, Up, Down }
 
 #[derive(Clone, Copy, Debug, PartialEq, Eq)]
-pub enum Key { Up, Down, Left, Right, Enter, Esc }
+pub enum Key { Up, Down, Left, Right, Enter, Esc, Space }
 
 #[derive(Clone, Copy, Debug, PartialEq, Eq)]
 #[cfg_attr(not(test), allow(dead_code))] // Done is not yet used by a toast; the herdr tests map it.
@@ -65,6 +65,7 @@
     ClosePane(String),
     CloseWorkspace(String),
     SendKeys { target: String, keys: Vec<Key> },
+    SendPaneKeys { target: String, keys: Vec<Key> },
     Prompt { target: String, text: String },
     Toast { title: String, body: String, sound: Sound },
     StartRecording,
@@ -92,6 +93,8 @@
     voice_button: Btn,
     /// The pane Respond was entered on; a press with another pane focused leaves Respond instead of answering.
     respond_pane: Option<String>,
+    /// Explicitly selected non-agent pane; an agent appearing invalidates this mode.
+    interact_pane: Option<String>,
     /// The user has pressed a key while this pane's agent was Blocked: the next A may answer the dialog.
     acknowledged_block: bool,
     recording: bool,
@@ -103,7 +106,7 @@
 
 impl Core {
     pub fn new(voice_button: Btn) -> Self {
-        Core { close_menu: None, mode: Mode::Navigate, voice_button, respond_pane: None, acknowledged_block: false, recording: false, transcribing: false, voice_target: None }
+        Core { close_menu: None, mode: Mode::Navigate, voice_button, respond_pane: None, interact_pane: None, acknowledged_block: false, recording: false, transcribing: false, voice_target: None }
     }
     pub fn mode(&self) -> Mode { self.mode }
     #[cfg_attr(not(test), allow(dead_code))]
@@ -215,6 +218,12 @@
 
     fn press(&mut self, b: Btn, snap: &Snapshot) -> Vec<Action> {
         let focused = snap.focused.as_ref();
+        if self.mode == Mode::Interact && !focused.is_some_and(|p| {
+            Some(&p.pane_id) == self.interact_pane.as_ref() && p.agent.is_none()
+        }) {
+            self.enter_navigate();
+            return vec![toast("Navigate mode", "pane changed", Sound::None)];
+        }
         let agent_pane = focused.filter(|p| p.agent.is_some());
         // Navigation works in both modes and leaves Respond.
         let nav = match b {
@@ -223,7 +232,7 @@
             Btn::Lt => Some(step_tab(snap, -1)),
             Btn::Rt => Some(step_tab(snap, 1)),
             Btn::Y => Some(next_attention(snap)),
-            Btn::X => Some(focused.map(|p| vec![Action::ToggleZoom(p.pane_id.clone())]).unwrap_or_default()),
+            Btn::X if self.mode != Mode::Interact => Some(focused.map(|p| vec![Action::ToggleZoom(p.pane_id.clone())]).unwrap_or_default()),
             _ => None,
         };
         if let Some(actions) = nav { return self.leave_respond(actions); }
@@ -249,11 +258,29 @@
                         if blocked { vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)] }
                         else { vec![toast("Respond mode", "D-pad moves; bottom confirms, right escapes; Back / Select leaves", Sound::None)] }
                     }
-                    None => vec![toast("No agent here", "focus a pane with an agent first", Sound::None)],
+                    None => match focused.filter(|p| !p.pane_id.is_empty()) {
+                        Some(p) => {
+                            self.mode = Mode::Interact;
+                            self.interact_pane = Some(p.pane_id.clone());
+                            vec![toast("Interact mode", "D-pad moves; Cross enters, Square selects, Circle goes back; Create leaves", Sound::None)]
+                        }
+                        None => vec![toast("No pane here", "focus a pane first", Sound::None)],
+                    },
                 },
                 _ => vec![],
             },
             Mode::Respond => self.respond(b, snap),
+            Mode::Interact => {
+                if b == Btn::View {
+                    self.enter_navigate();
+                    return vec![toast("Navigate mode", "", Sound::None)];
+                }
+                let key = if b == Btn::X { Some(Key::Space) } else { key_for(b) };
+                key.map(|key| vec![Action::SendPaneKeys {
+                    target: self.interact_pane.clone().expect("Interact has a validated target"),
+                    keys: vec![key],
+                }]).unwrap_or_default()
+            }
         }
     }
 
@@ -267,11 +294,12 @@
         self.cancel_close();
         self.mode = Mode::Navigate;
         self.respond_pane = None;
+        self.interact_pane = None;
         self.acknowledged_block = false;
     }
 
     fn leave_respond(&mut self, actions: Vec<Action>) -> Vec<Action> {
-        if self.mode != Mode::Respond { return actions; }
+        if self.mode == Mode::Navigate { return actions; }
         self.enter_navigate();
         let mut out = vec![toast("Navigate mode", "", Sound::None)];
         out.extend(actions);
--- a/src/herdr.rs
+++ b/src/herdr.rs
@@ -210,6 +210,11 @@
             v.extend(keys.iter().map(|k| key_name(*k).to_string()));
             v
         }
+        Action::SendPaneKeys { target, keys } => {
+            let mut v = s(&["herdr", "pane", "send-keys", target]);
+            v.extend(keys.iter().map(|k| key_name(*k).to_string()));
+            v
+        }
         Action::Prompt { target, text } => s(&["herdr", "agent", "prompt", target, text]),
         Action::Toast { title, body, sound } => {
             let mut v = s(&["herdr", "notification", "show", title]);
@@ -226,7 +231,7 @@
 }
 
 fn key_name(k: Key) -> &'static str {
-    match k { Key::Up => "up", Key::Down => "down", Key::Left => "left", Key::Right => "right", Key::Enter => "enter", Key::Esc => "esc" }
+    match k { Key::Up => "up", Key::Down => "down", Key::Left => "left", Key::Right => "right", Key::Enter => "enter", Key::Esc => "esc", Key::Space => "space" }
 }
 
 fn sound_name(s: Sound) -> &'static str {
--- a/src/serve.rs
+++ b/src/serve.rs
@@ -257,7 +257,7 @@
 Left / right trigger   Previous / next tab
 Triangle (Xbox: Y)     Next agent needing attention
 Square (Xbox: X)       Toggle pane zoom
-Cross (Xbox: A)        Enter Respond mode
+Cross (Xbox: A)        Enter Respond / Interact
 
 RESPOND
 D-pad                  Move through the agent's options
@@ -266,6 +266,18 @@
 Create / Share (View)  Return to Navigate
 Shoulders, triggers,
   Triangle / Square    Navigate as above and leave Respond
+
+INTERACT (NON-AGENT PANES)
+Cross enters this mode on the focused pane without sending a key.
+D-pad                  Send arrow keys to the pane
+Cross (Xbox: A)        Enter / open
+Square (Xbox: X)       Space / select
+Circle (Xbox: B)       Escape / back
+Create / Share (View)  Return to Navigate
+Shoulders, triggers,
+  Triangle             Navigate as above and leave Interact
+Focus changes or an agent appearing leave Interact without sending a key.
+PR Tracker: left/right choose categories; Square opens details; up/down move.
 
 CLOSE ACTIONS
 Menu / Options opens close actions; if voice uses Menu, use left stick click.
--- a/README.md
+++ b/README.md
@@ -62,9 +62,9 @@
 
 | Physical control | Xbox | PlayStation | Action |
 |---|---|---|---|
-| Bottom face | A | Cross | Enter Respond; confirm in Respond |
-| Right face | B | Circle | Escape in Respond |
-| Left face | X | Square | Toggle zoom |
+| Bottom face | A | Cross | Enter Respond / Interact; Enter when inside |
+| Right face | B | Circle | Escape in Respond / Interact |
+| Left face | X | Square | Toggle zoom; Space in Interact |
 | Top face | Y | Triangle | Next agent needing attention |
 | Left / right shoulder | LB / RB | L1 / R1 | Previous / next space |
 | Left / right trigger | LT / RT | L2 / R2 | Previous / next tab |
@@ -76,6 +76,16 @@
 agent, D-pad automatically enters Respond. Each confirmation requires acknowledgement
 first; press Cross (Xbox: A) again to confirm. Face-button positions always determine actions,
 even on controllers with different lettering.
+
+For a non-agent pane such as PR Tracker, press **Cross (Xbox: A)** to enter **Interact**.
+That first press only enters the mode. The D-pad sends arrow keys, **Cross (Xbox: A)**
+sends Enter, **Square (Xbox: X)** sends Space, and **Circle (Xbox: B)** sends Escape.
+In PR Tracker, left/right chooses a category, Square opens its details, and up/down
+moves through items. **Create / Share (Xbox: View)** returns to pane navigation;
+shoulders, triggers, and Triangle also leave Interact and keep their usual navigation actions.
+A focus change or an agent appearing in the pane exits Interact and consumes the next press.
+This mode works in ordinary terminal panes, including shells; the app decides what each key does.
+Dictation still requires an agent pane.
 
 Release the voice button to transcribe and automatically submit to an idle, done, or
 working agent—no extra Enter or controller press is needed. If focus changed, the pane
```

## Task 3: review and finish

Run the feature-loop fresh implementer, then independent spec and quality reviews. Run the final different-model herd-review. Do not merge or install until required reviews pass. Verify the pane adapter with the existing isolated raw-terminal receiver; no live PR action is needed. Have the user check controller focus -> Cross -> left/right category -> Square details -> up/down rows -> Circle back -> Create -> D-pad pane navigation. Do not claim this hardware path has been tested before the user validates it.

## Plan verification evidence

Scratch package: `/tmp/gamepad-interact-scratch` (archive of the pinned baseline, no production edits).

- `/tmp/interact-red.log`: six new feature tests failed assertions before implementation. `/tmp/interact-final-red.log` repeats the final patch against c8d8845: all seven interact_-matched tests fail assertions (six feature tests plus mapping).
- `/tmp/interact-mapping-red.log`: mapping assertion failed before text update. `/tmp/interact-final-mapping-red.log` also verifies the expanded row count fails (10 versus 14) before implementation.
- `/tmp/interact-final-green.log`: 157 unit + 4 integration tests passed.
- `/tmp/interact-build.log`: cargo build passed.
- `/tmp/interact-guard-{mode-validation,space-key,focus-id,agent-absent,nonempty-target,square-override,view-exit,global-exit}.log`: every explicit guard deletion failed assertions.
- `/tmp/interact-mutants.log` and `/tmp/interact-mutation/mutants.out/outcomes.json`: engine: 137 mutants = 121 caught + 16 unviable; pure adapter: 33 = 29 caught + 4 unviable. Required pure scope: 170 mutants, 150 caught, 20 unviable, 0 survivors. The broader run also tested 33 I/O mutations: 25 caught, 1 unviable, 7 survivors in unchanged `nonblocking` and `drain_available`; these unrelated I/O gaps are not claimed fixed. All counts derive from that single run; later baseline updates change only already-covered close-label strings and mapping alignment.
- Exact embedded patches were reapplied to `/tmp/interact-plan-final-assembled`, a clean c8d8845 archive, and matched scratch byte-for-byte. `/tmp/interact-assembled-green.log` and `/tmp/interact-assembled-build.log` record all 161 tests and build passing there.
