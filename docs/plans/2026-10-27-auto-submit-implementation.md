# Automatic Dictation Submission Implementation Plan

> Execute with the feature-loop subagent workflow, test first, then spec and quality reviews.

**Goal:** Submit dictated text automatically for Idle, Done and Working agents.

**Architecture:** Reuse Action::Prompt and its existing delivery/error handling. Remove
unused SendText action/adapter; preserve blank, focus, identity and status guards.
**Tech Stack:** Rust, Herdr CLI, cargo test and cargo-mutants.

## Task 1: Routing and public instructions

Files: src/engine.rs, src/engine_tests.rs, src/herdr.rs, src/herdr_tests.rs,
src/serve.rs, src/serve_tests.rs, README.md, CHANGELOG.md.

1. Update the Working transcript expectation and working delivery feedback test from
   SendText/Typed to Prompt/Sent. Run both and observe failure on the old implementation.
2. Apply the production changes below. Remove adapter tests for the removed SendText
   variant, preserving Prompt leading-dash literal-text coverage.
3. Add the button label regression below: full engine mutation testing found both
   constant label mutations survived. Assert this test fails with a constant label
   mutant, then restore the production function. No label behavior changes.
4. Update README: releasing the voice button automatically submits after transcription
   for idle/done/working agents; changed focus, blocked/unknown/no agent remain unsent.
   Add the same behavior change to Unreleased in CHANGELOG. Mapping text is in the patch.
5. Run cargo test --locked, cargo mutants --file src/engine.rs, and targeted argv_for
   mutations; release build, shell installer test, actionlint and git diff --check.
6. Spec compliance review, then code-quality review; different-family final review via
   herd-review. Fix concrete findings test-first. Install/restart the local daemon after
   verifying the release binary. Do not create a release tag or merge automatically.

## Exact assembled code delta

This delta was assembled in /tmp/herdr-gamepad-autosubmit-plan before implementation.
Tests failed on the old Working path, then 123 Rust tests passed with this code. The
label regression fails on the constant-label mutant. Final mutation results follow.

```diff
--- a/src/engine.rs
+++ b/src/engine.rs
@@ -64,7 +64,6 @@
     ToggleZoom(String),
     SendKeys { target: String, keys: Vec<Key> },
     Prompt { target: String, text: String },
-    SendText { pane_id: String, text: String },
     Toast { title: String, body: String, sound: Sound },
     StartRecording,
     StopRecording,
@@ -306,13 +305,9 @@
         return vec![toast("Not sent", &format!("no agent in the focused pane: {short}"), Sound::Request)];
     }
     match p.status {
-        Some(Status::Idle) | Some(Status::Done) => vec![
+        Some(Status::Idle) | Some(Status::Done) | Some(Status::Working) => vec![
             Action::Prompt { target: p.pane_id.clone(), text },
             toast("Sent", &short, Sound::None),
-        ],
-        Some(Status::Working) => vec![
-            Action::SendText { pane_id: p.pane_id.clone(), text },
-            toast("Typed, not sent", &format!("agent is working: {short}"), Sound::None),
         ],
         Some(Status::Blocked) => vec![toast("Not sent", &format!("agent is waiting at a dialog: {short}"), Sound::Request)],
         _ => vec![toast("Not sent", &format!("agent state unknown: {short}"), Sound::Request)],
--- a/src/engine_tests.rs
+++ b/src/engine_tests.rs
@@ -299,11 +299,11 @@
 }
 
 #[test]
-fn transcript_to_a_working_agent_is_typed_but_not_sent() {
+fn transcript_to_a_working_agent_is_submitted() {
     let s = Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Working))), ..snap() };
     assert_eq!(said("  also check lint  ", &s), vec![
-        Action::SendText { pane_id: "w5:p1".into(), text: "also check lint".into() },
-        toast("Typed, not sent", "agent is working: also check lint", Sound::None),
+        Action::Prompt { target: "w5:p1".into(), text: "also check lint".into() },
+        toast("Sent", "also check lint", Sound::None),
     ]);
 }
 
@@ -611,3 +611,18 @@
         assert_eq!(out[1], Action::SendKeys { target: "w5:p1".into(), keys: vec![key] }, "{b:?}");
     }
 }
+
+#[test]
+fn controller_button_labels_are_physical_and_distinct() {
+    let cases = [
+        (Btn::A, "bottom face button"), (Btn::B, "right face button"),
+        (Btn::X, "left face button"), (Btn::Y, "top face button"),
+        (Btn::Lb, "left shoulder"), (Btn::Rb, "right shoulder"),
+        (Btn::Lt, "left trigger"), (Btn::Rt, "right trigger"),
+        (Btn::View, "Back / Select"), (Btn::Menu, "Menu / Options"),
+        (Btn::Up, "D-pad up"), (Btn::Down, "D-pad down"),
+        (Btn::Left, "D-pad left"), (Btn::Right, "D-pad right"),
+        (Btn::LStick, "left stick click"), (Btn::RStick, "right stick click"),
+    ];
+    for (button, label) in cases { assert_eq!(button.label(), label); }
+}
--- a/src/herdr.rs
+++ b/src/herdr.rs
@@ -209,7 +209,6 @@
             v
         }
         Action::Prompt { target, text } => s(&["herdr", "agent", "prompt", target, text]),
-        Action::SendText { pane_id, text } => s(&["herdr", "pane", "send-text", pane_id, text]),
         Action::Toast { title, body, sound } => {
             let mut v = s(&["herdr", "notification", "show", title]);
             if !body.is_empty() { v.extend(s(&["--body", body])); }
--- a/src/herdr_tests.rs
+++ b/src/herdr_tests.rs
@@ -103,7 +103,6 @@
         (Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Esc] }, "herdr agent send-keys w5:p1 esc"),
         (Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Left, Key::Right, Key::Down] }, "herdr agent send-keys w5:p1 left right down"),
         (Action::Prompt { target: "w5:p1".into(), text: "run the tests".into() }, "herdr agent prompt w5:p1 run the tests"),
-        (Action::SendText { pane_id: "w5:p1".into(), text: "also lint".into() }, "herdr pane send-text w5:p1 also lint"),
         (toast("Sent", "run the tests", Sound::None), "herdr notification show Sent --body run the tests --sound none"),
         (toast("Not sent", "x", Sound::Request), "herdr notification show Not sent --body x --sound request"),
         (toast("Navigate mode", "", Sound::Done), "herdr notification show Navigate mode --sound done"),
@@ -168,12 +167,10 @@
     // Probed herdr 0.9.1 on 2026-10-27: `agent prompt`, `pane send-text` and `notification show --body` take "- hi there" and "--wait" as text, and a `--` before the text fails with "unknown option".
     for text in ["- hi there", "--wait"] {
         let prompt = argv_for(&Action::Prompt { target: "w5:p1".into(), text: text.into() });
-        let send_text = argv_for(&Action::SendText { pane_id: "w5:p1".into(), text: text.into() });
         let toast = argv_for(&toast("Sent", text, Sound::None));
         assert_eq!(prompt.last().map(String::as_str), Some(text), "{prompt:?}");
-        assert_eq!(send_text.last().map(String::as_str), Some(text), "{send_text:?}");
         assert_eq!(toast.iter().position(|a| a == "--body").map(|i| toast[i + 1].as_str()), Some(text), "{toast:?}");
-        for argv in [&prompt, &send_text, &toast] {
+        for argv in [&prompt, &toast] {
             assert!(!argv.iter().any(|a| a == "--"), "{argv:?}");
         }
     }
--- a/src/serve.rs
+++ b/src/serve.rs
@@ -85,7 +85,7 @@
                 crate::logging::event(format_args!("herd-gamepad: {other:?}: {e}"));
                 let _ = herdr.apply(&toast("Herdr refused", &e, Sound::Request));
                 // The remaining transcript action announces successful delivery.
-                if matches!(other, Action::Prompt { .. } | Action::SendText { .. }) { break; }
+                if matches!(other, Action::Prompt { .. }) { break; }
             },
         }
     }
@@ -199,7 +199,7 @@
 pub fn mapping() -> Result<(), String> {
     let (cfg, _) = crate::config::load()?;
     print!("{MAPPING}\nHold {} to talk; release to transcribe.\n", cfg.voice_button.label());
-    println!("Idle/done: sent. Working: typed without Enter. Blocked/unknown/no agent: dropped.");
+    println!("Idle/done/working: submitted automatically. Blocked/unknown/no agent: dropped.");
     println!("\nPress Enter to close.");
     let _ = std::io::stdin().read_line(&mut String::new());
     Ok(())
--- a/src/serve_tests.rs
+++ b/src/serve_tests.rs
@@ -38,7 +38,7 @@
         core.handle(Input::Press(Btn::RStick), &snap);
         core.handle(Input::Release(Btn::RStick), &snap);
         let actions = core.handle(Input::Transcript("hello".into()), &snap);
-        let command = if status == Status::Idle { "herdr agent prompt" } else { "herdr pane send-text" };
+        let command = "herdr agent prompt";
         let fake = Fake::new(vec![
             (command, if succeeds { Ok("") } else { Err("delivery failed") }),
             ("herdr notification show", Ok("")),
@@ -47,7 +47,7 @@
         let calls = fake.calls();
         assert_eq!(calls.len(), 2, "one delivery and exactly one outcome: {calls:?}");
         assert_eq!(calls[0], format!("{command} w5:p1 hello"));
-        let outcome = if !succeeds { "Herdr refused" } else if status == Status::Idle { "Sent" } else { "Typed, not sent" };
+        let outcome = if !succeeds { "Herdr refused" } else { "Sent" };
         assert!(calls[1].starts_with(&format!("herdr notification show {outcome} --body ")), "{calls:?}");
     }
 }
@@ -56,7 +56,7 @@
 fn prompt_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Idle); }
 
 #[test]
-fn typed_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Working); }
+fn working_prompt_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Working); }
 
 #[test]
 fn a_failed_mode_toast_does_not_discard_the_key_press() {
```

## Plan verification evidence

- Working routing and delivery tests failed against the old implementation.
- Assembled final code: 119 unit + 2 background + 2 manifest tests passed.
- Full engine mutation run: 98 mutants, 85 caught, 13 unviable, zero survivors/timeouts.
- Adapter argv_for mutation run: 4 mutants, all caught.
- Button label test failed against the previously surviving constant-label mutation.
