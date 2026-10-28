# Controller Button Labels Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Name the existing close controls clearly without changing any binding.

**Architecture:** Replace only the close-view and mapping strings, retaining physical names in diagnostic logs and the introductory mapping table. The existing voice-on-Menu fallback gets an explicit L3 cancellation label.

**Tech Stack:** Rust format strings, existing controller/core tests.

## One task group: visible copy and its regression tests

Files: `src/engine.rs`, `src/feedback_tests.rs`, `src/serve.rs`, `src/serve_tests.rs`, `README.md`.

1. Apply the test portions of the verified patch below first. Run `cargo test --locked close_`; the popup labels, fallback cancellation label, and mapping test must fail by assertions. Commit `test: require familiar controller button labels`.
2. Apply the production and README portions. No input matching, action dispatch, logging names, timing, or close safety changes are permitted. Run `cargo test --locked` and `cargo build --locked`.
3. Run `cargo mutants --file src/engine.rs --jobs 4 -- --bin herd-gamepad`. Replace the new cancellation-label conditional with each constant in turn; `cargo test --locked close_popup_names` must fail each time, then restore the source.
4. Commit `fix: name controller buttons clearly in close menus`. Fresh spec and quality reviews follow; parent performs the independent-model review and installation.

## Verification before commit

Scratch assembly: `/tmp/herdr-label-plan`, based on `cb875f4`. Three targeted tests failed on the old text, then 154 tests and build passed. Whole-engine mutation check: 127 tested, 111 caught, 16 unviable, no survivors/timeouts. Both cancellation-label branch deletions failed the positive label assertion. Representative popup content fits 62 interior columns and ten rows. Parent verified the installed daemon and user trace; user confirmed the existing Square → Cross sequence works.

## Verified patch

```diff
--- a/src/engine.rs
+++ b/src/engine.rs
@@ -161,11 +161,13 @@
     pub fn close_view(&self) -> Option<String> {
         let menu=self.close_menu.as_ref()?;
         let heading=match menu.choice {
-            None=>format!("Close actions — pane {} / space {}\nLeft face: close pane; top face: close space",menu.pane_id,menu.workspace_id),
-            Some(CloseKind::Pane)=>format!("Close pane {}?\nBottom face: confirm",menu.pane_id),
-            Some(CloseKind::Space)=>format!("Close space {}?\nBottom face: confirm",menu.workspace_id),
+            None=>format!("Close actions — pane {} / space {}\nSquare (Xbox: X): Close pane\nTriangle (Xbox: Y): Close space",menu.pane_id,menu.workspace_id),
+            Some(CloseKind::Pane)=>format!("Close pane {}?\nCross (Xbox: A): Confirm",menu.pane_id),
+            Some(CloseKind::Space)=>format!("Close space {}?\nCross (Xbox: A): Confirm",menu.workspace_id),
         };
-        Some(format!("{heading}\nStops its running processes.\nRight face / Back / {}: cancel",menu_button(self.voice_button).label()))
+        let cancel_menu = if self.voice_button == Btn::Menu { "L3 / left stick click" }
+                          else { "Options (Xbox: Menu)" };
+        Some(format!("{heading}\nStops its running processes.\nCircle (Xbox: B): Cancel\nCreate / Share (Xbox: View): Cancel\n{cancel_menu}: Cancel"))
     }
 
 
--- a/src/feedback_tests.rs
+++ b/src/feedback_tests.rs
@@ -302,7 +302,7 @@
     assert!(core
         .close_view()
         .unwrap()
-        .contains("Right face / Back / left stick click: cancel"));
+        .contains("L3 / left stick click: Cancel"));
 }
 
 #[test]
@@ -377,3 +377,33 @@
     fs::remove_file(root.join("lock")).unwrap();
     fs::remove_dir(root).unwrap();
 }
+
+#[test]
+fn close_popup_names_playstation_buttons_with_explicit_xbox_equivalents() {
+    let snap = crate::engine_tests::snap();
+    for voice in [Btn::RStick, Btn::LStick, Btn::Menu] {
+        let mut core = Core::new(voice);
+        core.handle(Input::Press(menu_button(voice)), &snap);
+        let view = core.close_view().unwrap();
+        for label in ["Square (Xbox: X): Close pane", "Triangle (Xbox: Y): Close space",
+                      "Circle (Xbox: B): Cancel", "Create / Share (Xbox: View): Cancel"] {
+            assert!(view.contains(label), "missing {label}: {view}");
+        }
+        let cancel = if voice == Btn::Menu { "L3 / left stick click: Cancel" }
+                     else { "Options (Xbox: Menu): Cancel" };
+        assert!(view.contains(cancel));
+        for choice in [Btn::X, Btn::Y] {
+            core.handle(Input::Press(choice), &snap);
+            let selected = core.close_view().unwrap();
+            assert!(selected.contains("Cross (Xbox: A): Confirm"));
+            assert!(selected.contains("Circle (Xbox: B): Cancel"));
+            assert!(selected.contains(cancel));
+            assert!(!selected.contains("face"));
+            assert!(selected.lines().count() <= 10);
+            assert!(selected.lines().all(|line| line.chars().count() <= 62));
+        }
+        assert!(!view.contains("face"));
+        assert!(view.lines().count() <= 10);
+        assert!(view.lines().all(|line| line.chars().count() <= 62));
+    }
+}
--- a/src/serve.rs
+++ b/src/serve.rs
@@ -255,29 +255,29 @@
 D-pad                  Focus a neighbouring pane
 Left / right shoulder  Previous / next space
 Left / right trigger   Previous / next tab
-Top face button        Next agent needing attention
-Left face button       Toggle pane zoom
-Bottom face button     Enter Respond mode
+Triangle (Xbox: Y)      Next agent needing attention
+Square (Xbox: X)        Toggle pane zoom
+Cross (Xbox: A)         Enter Respond mode
 
 RESPOND
 D-pad                  Move through the agent's options
-Bottom face button     Enter / confirm
-Right face button      Escape
-Back / Select          Return to Navigate
+Cross (Xbox: A)         Enter / confirm
+Circle (Xbox: B)        Escape
+Create / Share (View)   Return to Navigate
 Shoulders, triggers,
-  top or left face     Navigate as above and leave Respond
+  Triangle / Square    Navigate as above and leave Respond
 
 CLOSE ACTIONS
 Menu / Options opens close actions; if voice uses Menu, use left stick click.
-Left face button       Select close pane
-Top face button        Select close space
-Bottom face button     Confirm selected close
-Right face / Back      Cancel; pressing the menu button again also cancels
+Square (Xbox: X)        Select close pane
+Triangle (Xbox: Y)      Select close space
+Cross (Xbox: A)         Confirm selected close
+Circle (Xbox: B)        Cancel; Back or the menu button also cancels
 Closing stops running processes in the pane or space. Focus changes cancel.
 Finish dictation before opening close actions.
 
 A D-pad press on a blocked agent auto-switches to Respond.
-Each blocked-agent confirmation needs acknowledgement first; press bottom again.
+Each blocked-agent confirmation needs acknowledgement first; press Cross (Xbox: A) again.
 Routine status updates are in Gamepad logs. Errors are shown at most once per 5 seconds.
 Close actions use a live popup; wait for the selected target to appear before confirming.
 Button positions define the actions, even when your controller uses other labels.
--- a/src/serve_tests.rs
+++ b/src/serve_tests.rs
@@ -102,7 +102,7 @@
 
 #[test]
 fn mapping_explains_close_selection_confirmation_and_voice_collision() {
-    for text in ["CLOSE ACTIONS", "Left face button       Select close pane", "Top face button        Select close space", "Bottom face button     Confirm selected close", "Menu / Options opens close actions; if voice uses Menu, use left stick click."] {
+    for text in ["CLOSE ACTIONS", "Square (Xbox: X)        Select close pane", "Triangle (Xbox: Y)      Select close space", "Cross (Xbox: A)         Confirm selected close", "Menu / Options opens close actions; if voice uses Menu, use left stick click."] {
         assert!(MAPPING.contains(text), "mapping is missing {text}");
     }
 }
--- a/README.md
+++ b/README.md
@@ -4,7 +4,8 @@
 answer agent prompts, and dictate through whisper.cpp on your machine.
 
 Controls follow physical button positions. The mapping card shows Xbox and PlayStation
-names side by side; prompts and logs use neutral names such as "bottom face button".
+names side by side. The close popup names PlayStation buttons with explicit Xbox equivalents;
+diagnostic logs retain physical names such as "bottom face button".
 Controller compatibility still depends on macOS and the input backend. A DualSense has
 been exercised locally; other controller models need their own hardware checks.
 
@@ -73,7 +74,7 @@
 
 D-pad moves between panes in Navigate, and between options in Respond. On a blocked
 agent, D-pad automatically enters Respond. Each confirmation requires acknowledgement
-first; press the bottom face button again to confirm. Face-button positions always determine actions,
+first; press Cross (Xbox: A) again to confirm. Face-button positions always determine actions,
 even on controllers with different lettering.
 
 Release the voice button to transcribe and automatically submit to an idle, done, or
@@ -82,9 +83,9 @@
 is not sent.
 
 To close a pane or space, press **Menu / Options** to open the live close-actions popup. Press the
-**left face button (X / Square)** to select Close pane, or the **top face button
-(Y / Triangle)** to select Close space. Wait for the selected target to appear in the popup, then press the
-**bottom face button (A / Cross)** to confirm. **Right face (B / Circle)**,
+**Square (Xbox: X)** to select Close pane, or **Triangle (Xbox: Y)** to select Close space.
+Wait for the selected target to appear in the popup, then press **Cross (Xbox: A)** to confirm.
+**Circle (Xbox: B)**,
 **Back / Select**, or the menu button cancels. Closing stops running processes in that
 pane or space; closing a space includes all its panes and tabs, not its workspace group.
 Closing the popup, a focus change, or a controller disconnect cancels the pending action. Finish dictation
```
