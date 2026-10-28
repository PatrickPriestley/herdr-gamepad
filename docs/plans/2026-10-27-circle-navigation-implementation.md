# Circle returns to pane navigation implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Circle consistently leave Respond/Interact and let the D-pad move away from blocked agents.

**Architecture:** Circle exits either interaction mode without sending a terminal key. Create/Share sends Escape through each mode's existing agent/pane route and retains Respond acknowledgement semantics. Navigate always routes D-pad to pane focus; entering Respond requires Cross even on blocked agents.

**Tech Stack:** Existing Rust state machine, Herdr adapter, cargo test/mutants.
**Baseline:** `29c44e5173204696c91b1e20c2223a694c1a5df8`.

## Verified problem and decisions

The user asked for Circle to return to navigation. The recommended Create/Share mapping is Escape inside the app/agent. Circle does nothing in Navigate. Cross entry/Enter and Square Space in Interact remain unchanged. The close menu still uses both Circle and Create/Share to cancel.

An isolated engine replay against the baseline proved the existing navigation trap independently of the new Circle behavior: Cross entered Respond on a blocked agent, View exited to Navigate, then Down produced a Respond toast and agent SendKeys instead of FocusDirection. Evidence: `/tmp/circle-existing-exit-trap-red.log`. Therefore remove automatic blocked-agent D-pad entry and its now-unused key_of helper. All explicit Cross/blocked acknowledgement safeguards remain.

## Task 1: tests first

**Files:** `src/engine_tests.rs`, `src/herdr_tests.rs`, `src/serve_tests.rs`.

Apply this exact patch, then run `cargo test --locked --quiet`. The changed controls and new assertions must fail against the baseline. Existing tests for the old automatic D-pad entry and View exit are intentionally rewritten; unrelated close-menu cancel tests remain intact. The quiet feedback test now explicitly enters Respond before sending its arrow, preserving its actual feedback coverage.

Commit with `test: specify Circle exit and predictable pane navigation`.

```diff
--- a/src/engine_tests.rs
+++ b/src/engine_tests.rs
@@ -171,17 +171,17 @@
     assert_eq!(c.handle(Input::Press(Btn::Left), &snap()), keys("w5:p1", Key::Left));
     assert_eq!(c.handle(Input::Press(Btn::Right), &snap()), keys("w5:p1", Key::Right));
     assert_eq!(c.handle(Input::Press(Btn::A), &snap()), keys("w5:p1", Key::Enter));
-    assert_eq!(c.handle(Input::Press(Btn::B), &snap()), keys("w5:p1", Key::Esc));
+    assert_eq!(c.handle(Input::Press(Btn::View), &snap()), keys("w5:p1", Key::Esc));
     assert_eq!(c.mode(), Mode::Respond);
 }
 
 #[test]
-fn view_leaves_respond_mode() {
-    let mut c = in_respond();
-    let out = c.handle(Input::Press(Btn::View), &snap());
+fn circle_leaves_respond_mode() {
+    let mut c = in_respond();
+    let out = c.handle(Input::Press(Btn::B), &snap());
     assert_eq!(c.mode(), Mode::Navigate);
     assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "Navigate mode"), "{out:?}");
-    assert_eq!(c.handle(Input::Press(Btn::View), &snap()), vec![], "View in Navigate does nothing");
+    assert_eq!(c.handle(Input::Press(Btn::B), &snap()), vec![], "Circle in Navigate does nothing");
 }
 
 #[test]
@@ -222,16 +222,11 @@
 }
 
 #[test]
-fn dpad_on_a_blocked_agent_switches_to_respond_and_sends_the_key() {
-    let mut c = core();
-    let out = c.handle(Input::Press(Btn::Down), &blocked());
-    assert_eq!(c.mode(), Mode::Respond);
-    assert_eq!(out, vec![
-        toast("Respond mode", "the agent is waiting for you", Sound::None),
-        Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Down] },
-    ]);
-    // The press that switched modes was aimed at moving focus, not at the dialog: it is no acknowledgement.
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
+fn dpad_on_a_blocked_agent_stays_in_navigate_until_cross_enters() {
+    let mut c = core();
+    assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Down }]);
+    assert_eq!(c.mode(), Mode::Navigate);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)]);
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
 }
 
@@ -432,9 +427,9 @@
 }
 
 #[test]
-fn b_on_a_fresh_dialog_is_sent_and_counts_as_the_acknowledgement() {
-    let mut c = in_respond();
-    assert_eq!(c.handle(Input::Press(Btn::B), &blocked()), keys("w5:p1", Key::Esc));
+fn share_on_a_fresh_dialog_is_sent_and_counts_as_the_acknowledgement() {
+    let mut c = in_respond();
+    assert_eq!(c.handle(Input::Press(Btn::View), &blocked()), keys("w5:p1", Key::Esc));
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
 }
 
@@ -493,12 +488,12 @@
 fn leaving_respond_forgets_the_acknowledgement() {
     let mut c = in_respond();
     c.handle(Input::Press(Btn::Down), &blocked());
-    c.handle(Input::Press(Btn::View), &blocked());
+    c.handle(Input::Press(Btn::B), &blocked());
     // Re-entering with A on the blocked pane: that A is the acknowledgement, the next confirms.
     c.handle(Input::Press(Btn::A), &blocked());
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
     // Re-entering with A on an idle pane that then blocks: the first A is swallowed.
-    c.handle(Input::Press(Btn::View), &snap());
+    c.handle(Input::Press(Btn::B), &snap());
     c.handle(Input::Press(Btn::A), &snap());
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
 }
@@ -594,11 +589,11 @@
 }
 
 #[test]
-fn the_auto_switch_sends_every_direction() {
-    for (b, key) in [(Btn::Up, Key::Up), (Btn::Down, Key::Down), (Btn::Left, Key::Left), (Btn::Right, Key::Right)] {
+fn navigate_on_a_blocked_agent_moves_focus_in_every_direction() {
+    for (b, dir) in [(Btn::Up, Dir::Up), (Btn::Down, Dir::Down), (Btn::Left, Dir::Left), (Btn::Right, Dir::Right)] {
         let mut c = core();
-        let out = c.handle(Input::Press(b), &blocked());
-        assert_eq!(out[1], Action::SendKeys { target: "w5:p1".into(), keys: vec![key] }, "{b:?}");
+        assert_eq!(c.handle(Input::Press(b), &blocked()), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir }]);
+        assert_eq!(c.mode(),Mode::Navigate);
     }
 }
 
@@ -749,7 +744,7 @@
 #[test]
 fn interact_enters_without_sending_and_maps_all_keys() {
     let mut c = in_interact();
-    for (button, key) in [(Btn::Up,"Up"),(Btn::Down,"Down"),(Btn::Left,"Left"),(Btn::Right,"Right"),(Btn::A,"Enter"),(Btn::B,"Esc"),(Btn::X,"Space")] {
+    for (button, key) in [(Btn::Up,"Up"),(Btn::Down,"Down"),(Btn::Left,"Left"),(Btn::Right,"Right"),(Btn::A,"Enter"),(Btn::View,"Esc"),(Btn::X,"Space")] {
         assert_eq!(format!("{:?}", c.handle(Input::Press(button), &non_agent())), format!("[SendPaneKeys {{ target: \"w5:p2\", keys: [{key}] }}]"));
         assert_eq!(format!("{:?}", c.mode()), "Interact");
     }
@@ -781,8 +776,8 @@
     }
 }
 #[test]
-fn interact_view_disconnect_and_global_navigation_exit() {
-    for b in [Btn::View,Btn::Lb,Btn::Rb,Btn::Lt,Btn::Rt,Btn::Y] {
+fn interact_circle_disconnect_and_global_navigation_exit() {
+    for b in [Btn::B,Btn::Lb,Btn::Rb,Btn::Lt,Btn::Rt,Btn::Y] {
         let mut c=in_interact();
         let expected=core().handle(Input::Press(b),&non_agent());
         let out=c.handle(Input::Press(b),&non_agent());
@@ -801,3 +796,52 @@
     c.handle(Input::Press(Btn::X),&non_agent());
     assert_eq!(c.handle(Input::Press(Btn::A),&non_agent()),vec![Action::ClosePane("w5:p2".into())]);
 }
+
+#[test]
+fn circle_exits_both_modes_without_sending_and_preserves_safe_reentry() {
+    for s in [snap(), blocked(), non_agent()] {
+        let mut c=core(); c.handle(Input::Press(Btn::A), &s);
+        assert_ne!(c.mode(),Mode::Navigate);
+        assert_eq!(c.handle(Input::Press(Btn::B),&s),vec![toast("Navigate mode","",Sound::None)]);
+        assert_eq!(c.mode(),Mode::Navigate);
+        assert_eq!(c.handle(Input::Press(Btn::B),&s),vec![]);
+        let entry=c.handle(Input::Press(Btn::A),&s);
+        assert!(matches!(entry.as_slice(),[Action::Toast{..}]));
+        assert_ne!(c.mode(),Mode::Navigate);
+    }
+}
+#[test]
+fn circle_focus_mismatch_never_sends_a_key() {
+    let mut positive=in_respond();
+    assert_eq!(positive.handle(Input::Press(Btn::B),&snap()),vec![toast("Navigate mode","",Sound::None)]);
+    for mut c in [in_respond(),in_interact()] {
+        let changed=Snapshot{focused:None,..snap()};
+        let out=c.handle(Input::Press(Btn::B),&changed);
+        assert_eq!(c.mode(),Mode::Navigate);
+        assert!(matches!(out.as_slice(),[Action::Toast{title,..}] if title=="Navigate mode"));
+    }
+}
+#[test]
+fn share_escape_keeps_the_mode_and_uses_its_own_route() {
+    let mut c=in_respond();
+    assert_eq!(c.handle(Input::Press(Btn::View),&blocked()),keys("w5:p1",Key::Esc));
+    assert_eq!(c.mode(),Mode::Respond);
+    assert_eq!(c.handle(Input::Press(Btn::A),&blocked()),keys("w5:p1",Key::Enter));
+    assert!(matches!(c.handle(Input::Press(Btn::A),&blocked()).as_slice(),[Action::Toast{..}]));
+    let mut c=in_interact();
+    assert_eq!(c.handle(Input::Press(Btn::View),&non_agent()),vec![Action::SendPaneKeys{target:"w5:p2".into(),keys:vec![Key::Esc]}]);
+    assert_eq!(c.mode(),Mode::Interact);
+    assert_eq!(core().handle(Input::Press(Btn::View),&snap()),vec![]);
+}
+
+#[test]
+fn navigate_dpad_does_not_get_trapped_by_a_blocked_agent() {
+    let mut c=core();
+    assert_eq!(c.handle(Input::Press(Btn::Down),&blocked()),vec![Action::FocusDirection{pane_id:"w5:p1".into(),dir:Dir::Down}]);
+    assert_eq!(c.mode(),Mode::Navigate);
+    c.handle(Input::Press(Btn::A),&blocked());
+    assert_eq!(c.mode(),Mode::Respond);
+    c.handle(Input::Press(Btn::B),&blocked());
+    assert_eq!(c.handle(Input::Press(Btn::Down),&blocked()),vec![Action::FocusDirection{pane_id:"w5:p1".into(),dir:Dir::Down}]);
+    assert_eq!(c.mode(),Mode::Navigate);
+}
--- a/src/herdr_tests.rs
+++ b/src/herdr_tests.rs
@@ -276,7 +276,7 @@
     let mut core = Core::new(Btn::RStick);
     core.handle(Input::Press(Btn::A), &snap);
     assert_eq!(core.mode(), Mode::Respond);
-    core.handle(Input::Press(Btn::View), &snap);
+    core.handle(Input::Press(Btn::B), &snap);
     assert_eq!(core.mode(), Mode::Navigate);
 }
 
@@ -299,7 +299,7 @@
     let mut c=Core::new(Btn::RStick);
     let s=Snapshot { focused: Some(Pane{pane_id:"w5:p2".into(),..Pane::default()}),..Snapshot::default() };
     c.handle(Input::Press(Btn::A),&s);
-    for (b,key) in [(Btn::Up,"up"),(Btn::Down,"down"),(Btn::Left,"left"),(Btn::Right,"right"),(Btn::A,"enter"),(Btn::B,"esc"),(Btn::X,"space")] {
+    for (b,key) in [(Btn::Up,"up"),(Btn::Down,"down"),(Btn::Left,"left"),(Btn::Right,"right"),(Btn::A,"enter"),(Btn::View,"esc"),(Btn::X,"space")] {
         let actions=c.handle(Input::Press(b),&s);
         assert_eq!(actions.len(),1);
         assert_eq!(argv_for(&actions[0]),vec!["herdr","pane","send-keys","w5:p2",key]);
--- a/src/serve_tests.rs
+++ b/src/serve_tests.rs
@@ -64,7 +64,9 @@
     use crate::herdr_tests::Fake;
     let mut snap = crate::engine_tests::snap();
     snap.focused.as_mut().unwrap().status = Some(Status::Blocked);
-    let actions = Core::new(Btn::RStick).handle(Input::Press(Btn::Down), &snap);
+    let mut core = Core::new(Btn::RStick);
+    let mut actions = core.handle(Input::Press(Btn::A), &snap);
+    actions.extend(core.handle(Input::Press(Btn::Down), &snap));
     let fake = Fake::new(vec![
         ("herdr notification show", Err("notifications unavailable")),
         ("herdr agent send-keys", Ok("")),
@@ -80,7 +82,7 @@
     assert_eq!(c.handle(Input::Press(Btn::Menu), &s), vec![toast("Close actions", "left face: close pane; top face: close space; right face: cancel", Sound::None)]);
     assert!(matches!(c.handle(Input::Press(Btn::X), &s).first(), Some(Action::Toast { title, .. }) if title == "Confirm close"));
     on_snapshot_failure(&mut c, Input::Press(Btn::A), "offline");
-    assert_eq!(c.handle(Input::Press(Btn::A), &s), vec![toast("Respond mode", "D-pad moves; bottom confirms, right escapes; Back / Select leaves", Sound::None)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &s), vec![toast("Respond mode", "D-pad moves; Cross confirms; Circle returns to Navigate; Create / Share escapes", Sound::None)]);
 }
 
 #[test]
@@ -204,3 +206,9 @@
         assert!(MAPPING.contains(text), "mapping is missing {text}");
     }
 }
+
+#[test]
+fn mapping_distinguishes_circle_navigation_from_share_escape() {
+    assert_eq!(MAPPING.matches("Circle (Xbox: B)       Return to Navigate").count(),2);
+    assert_eq!(MAPPING.matches("Create / Share (View)  Escape / back in the app").count(),2);
+}
```

## Task 2: routing and documentation

**Files:** `src/engine.rs`, `src/serve.rs`, `README.md`.

Apply the exact patch below. Run `cargo test --locked --quiet` and `cargo build --locked`: all 166 tests and build must pass. Run `cargo mutants --file src/engine.rs --jobs 4 --output /tmp/circle-mutation-check`. Delete the Circle exit guard separately in Respond and Interact; each deletion must fail assertions. Revert Share's Escape mapping to Circle and verify failure as well. Restore source after every mutation.

Commit with `fix: return to pane navigation with Circle`.

```diff
--- a/src/engine.rs
+++ b/src/engine.rs
@@ -237,15 +237,6 @@
         };
         if let Some(actions) = nav { return self.leave_respond(actions); }
         let blocked = agent_pane.is_some_and(|p| p.status == Some(Status::Blocked));
-        if self.mode == Mode::Navigate && blocked && matches!(b, Btn::Up | Btn::Down | Btn::Left | Btn::Right) {
-            // The press was aimed at moving focus, not at the dialog: the arrow goes through but is no
-            // acknowledgement, so the next A is swallowed once. `respond` is bypassed because it would count it.
-            self.enter_respond(agent_pane, false);
-            return vec![
-                toast("Respond mode", "the agent is waiting for you", Sound::None),
-                Action::SendKeys { target: agent_pane.map(|p| p.pane_id.clone()).unwrap_or_default(), keys: vec![key_of(b)] },
-            ];
-        }
         match self.mode {
             Mode::Navigate => match b {
                 Btn::Up | Btn::Down | Btn::Left | Btn::Right => focused
@@ -256,13 +247,13 @@
                         // Entering on a blocked agent is the acknowledgement; the next A confirms.
                         self.enter_respond(agent_pane, blocked);
                         if blocked { vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)] }
-                        else { vec![toast("Respond mode", "D-pad moves; bottom confirms, right escapes; Back / Select leaves", Sound::None)] }
+                        else { vec![toast("Respond mode", "D-pad moves; Cross confirms; Circle returns to Navigate; Create / Share escapes", Sound::None)] }
                     }
                     None => match focused.filter(|p| !p.pane_id.is_empty()) {
                         Some(p) => {
                             self.mode = Mode::Interact;
                             self.interact_pane = Some(p.pane_id.clone());
-                            vec![toast("Interact mode", "D-pad moves; Cross enters, Square selects, Circle goes back; Create leaves", Sound::None)]
+                            vec![toast("Interact mode", "D-pad moves; Cross enters; Square selects; Circle returns to Navigate; Create / Share escapes", Sound::None)]
                         }
                         None => vec![toast("No pane here", "focus a pane first", Sound::None)],
                     },
@@ -271,7 +262,7 @@
             },
             Mode::Respond => self.respond(b, snap),
             Mode::Interact => {
-                if b == Btn::View {
+                if b == Btn::B {
                     self.enter_navigate();
                     return vec![toast("Navigate mode", "", Sound::None)];
                 }
@@ -314,7 +305,7 @@
             self.enter_navigate();
             return vec![toast("Navigate mode", "focus moved to another pane", Sound::None)];
         }
-        if b == Btn::View {
+        if b == Btn::B {
             self.enter_navigate();
             return vec![toast("Navigate mode", "", Sound::None)];
         }
@@ -341,14 +332,9 @@
 fn key_for(b: Btn) -> Option<Key> {
     Some(match b {
         Btn::Up => Key::Up, Btn::Down => Key::Down, Btn::Left => Key::Left, Btn::Right => Key::Right,
-        Btn::A => Key::Enter, Btn::B => Key::Esc,
+        Btn::A => Key::Enter, Btn::View => Key::Esc,
         _ => return None,
     })
-}
-
-/// The key a D-pad button sends.
-fn key_of(b: Btn) -> Key {
-    match b { Btn::Up => Key::Up, Btn::Down => Key::Down, Btn::Left => Key::Left, _ => Key::Right }
 }
 
 fn dir_of(b: Btn) -> Dir {
--- a/src/serve.rs
+++ b/src/serve.rs
@@ -262,8 +262,8 @@
 RESPOND
 D-pad                  Move through the agent's options
 Cross (Xbox: A)        Enter / confirm
-Circle (Xbox: B)       Escape
-Create / Share (View)  Return to Navigate
+Circle (Xbox: B)       Return to Navigate
+Create / Share (View)  Escape / back in the app
 Shoulders, triggers,
   Triangle / Square    Navigate as above and leave Respond
 
@@ -272,8 +272,8 @@
 D-pad                  Send arrow keys to the pane
 Cross (Xbox: A)        Enter / open
 Square (Xbox: X)       Space / select
-Circle (Xbox: B)       Escape / back
-Create / Share (View)  Return to Navigate
+Circle (Xbox: B)       Return to Navigate
+Create / Share (View)  Escape / back in the app
 Shoulders, triggers,
   Triangle             Navigate as above and leave Interact
 Focus changes or an agent appearing leave Interact without sending a key.
@@ -288,7 +288,7 @@
 Closing stops running processes in the pane or space. Focus changes cancel.
 Finish dictation before opening close actions.
 
-A D-pad press on a blocked agent auto-switches to Respond.
+D-pad always moves pane focus in Navigate; Cross enters Respond on an agent.
 Each blocked-agent confirmation needs acknowledgement first; press Cross (Xbox: A) again.
 Routine status updates are in Gamepad logs. Errors are shown at most once per 5 seconds.
 Close actions use a live popup; wait for the selected target to appear before confirming.
--- a/README.md
+++ b/README.md
@@ -63,26 +63,27 @@
 | Physical control | Xbox | PlayStation | Action |
 |---|---|---|---|
 | Bottom face | A | Cross | Enter Respond / Interact; Enter when inside |
-| Right face | B | Circle | Escape in Respond / Interact |
+| Right face | B | Circle | Return to Navigate |
 | Left face | X | Square | Toggle zoom; Space in Interact |
 | Top face | Y | Triangle | Next agent needing attention |
 | Left / right shoulder | LB / RB | L1 / R1 | Previous / next space |
 | Left / right trigger | LT / RT | L2 / R2 | Previous / next tab |
-| Back / Select | View | Create (PS5) / Share (PS4) | Return to Navigate |
+| Back / Select | View | Create (PS5) / Share (PS4) | Escape in Respond / Interact |
 | Right stick click | Right stick click | R3 | Hold to dictate (default) |
 | Menu / Options | Menu | Options | Open close actions |
 
-D-pad moves between panes in Navigate, and between options in Respond. On a blocked
-agent, D-pad automatically enters Respond. Each confirmation requires acknowledgement
+D-pad moves between panes in Navigate, even when an agent is blocked. Press Cross
+(Xbox: A) to enter Respond and move between its options. Each confirmation requires acknowledgement
 first; press Cross (Xbox: A) again to confirm. Face-button positions always determine actions,
 even on controllers with different lettering.
 
 For a non-agent pane such as PR Tracker, press **Cross (Xbox: A)** to enter **Interact**.
 That first press only enters the mode. The D-pad sends arrow keys, **Cross (Xbox: A)**
-sends Enter, **Square (Xbox: X)** sends Space, and **Circle (Xbox: B)** sends Escape.
+sends Enter, **Square (Xbox: X)** sends Space, and **Create / Share (Xbox: View)** sends
+Escape to go back inside the app. **Circle (Xbox: B)** returns to pane navigation
+without sending a key. The same Circle/Share distinction applies in Respond.
 In PR Tracker, left/right chooses a category, Square opens its details, and up/down
-moves through items. **Create / Share (Xbox: View)** returns to pane navigation;
-shoulders, triggers, and Triangle also leave Interact and keep their usual navigation actions.
+moves through items. Shoulders, triggers, and Triangle also leave Interact and keep their usual navigation actions.
 A focus change or an agent appearing in the pane exits Interact and consumes the next press.
 This mode works in ordinary terminal panes, including shells; the app decides what each key does.
 Dictation still requires an agent pane.
```

## Task 3: review and hardware validation

Follow fresh implementation, spec review, quality review, and final different-model review before installation. Validate on the controller: focus blocked agent -> Cross -> Circle -> D-pad changes pane; focus PR Tracker -> Cross -> arrows/Square navigate -> Share goes back within app -> Circle -> D-pad changes pane. Cross must never send Enter merely by entering a mode, and Circle must never answer a blocked-agent dialog.

## Verification evidence

Scratch: `/tmp/gamepad-circle-scratch`; no production edits or commits.

- `/tmp/circle-existing-exit-trap-red.log`: baseline navigation trap reproduced with the old View exit.
- `/tmp/circle-navigation-trap-red.log`: permanent Navigate regression fails with actual SendKeys versus expected FocusDirection.
- `/tmp/circle-final-tests-red.log`: test expectations failed before implementation. `/tmp/circle-assembled-red.log` replays the exact final test patch on baseline: 16 failing assertions, 146 passing tests.
- `/tmp/circle-green.log`: 162 unit + 4 integration tests pass.
- `/tmp/circle-build.log`: build passes. The embedded patches also reproduce scratch byte-for-byte in `/tmp/circle-plan-assembled`; `/tmp/circle-assembled-green.log` and `/tmp/circle-assembled-build.log` confirm all 166 tests and build pass there. Both patches pass `git apply --check` in the production feature worktree.
- `/tmp/circle-guard-{interact-circle-exit,respond-circle-exit,share-escape}.log`: guard/key-mapping mutations fail assertions.
- `/tmp/circle-mutants.log` and `/tmp/circle-mutation/mutants.out/outcomes.json`: all 130 engine mutants tested, 115 caught, 15 unviable, zero survivors or timeouts. The adapter is unchanged by this delta.
