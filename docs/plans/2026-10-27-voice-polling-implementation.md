# Voice Polling Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Deliver completed dictation without needing another controller button event.

**Architecture:** Keep controller and voice dispatch in the daemon thread, polling gilrs nonblockingly once per 10 ms tick. A small `next_inputs` seam drains completed transcripts before a new button input and throttles idle work.

**Tech Stack:** Rust, gilrs 0.11.2, std mpsc and sleep.

---

## Task 1: Record the regression before fixing the adapter

Files: `src/serve.rs`, `src/serve_tests.rs`.

First extract the current block to `next_inputs`, with the controller closure still invoking `gilrs.next_event_blocking(Some(Duration::from_millis(100)))`. Initially the helper must collect `poll()` first, then extend `rx.try_iter()`, without sleep. Add the three tests in the final patch below. Run `cargo test --locked serve_tests`: all three new scenarios fail by assertions (nonblocking adapter, ordered transcript, throttle). This exact RED assembly was run in scratch.

Commit the regression as `test: reproduce voice results waiting on controller input`.

## Task 2: Apply the verified final implementation

Use the following exact final diff relative to the pre-extraction base. If integrating after the close-controls work, preserve its independent `on_snapshot_failure` changes and mapping text; only the event acquisition block and new helper change here.

```diff
--- a/src/serve.rs
+++ b/src/serve.rs
@@ -29,11 +29,10 @@
     let (tx, rx) = mpsc::channel::<Input>();
     crate::logging::event(format_args!("herd-gamepad: listening; {} controller(s) connected; hold {} to talk", gilrs.gamepads().count(), cfg.voice_button.label()));
     while !cancellation.cancelled() {
-        let mut inputs = vec![];
-        if let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(100))) {
-            if let Some(i) = pad::raw(&ev.event).and_then(|r| pad.input(r)) { inputs.push(i); }
-        }
-        inputs.extend(rx.try_iter());
+        let inputs = next_inputs(|| {
+            gilrs.next_event()
+                .and_then(|ev| pad::raw(&ev.event)).and_then(|r| pad.input(r))
+        }, &rx);
         for mut input in inputs {
             if cancellation.cancelled() { break; }
             let snap = match needs_snapshot(&input, cfg.voice_button).then(|| herdr.snapshot()) {
@@ -76,6 +75,17 @@
     Ok(())
 }
 
+/// One daemon tick: controller polling and completed transcriptions.
+pub fn next_inputs(poll: impl FnOnce() -> Option<Input>, rx: &mpsc::Receiver<Input>) -> Vec<Input> {
+    // A finished transcript clears transcribing before a simultaneous new voice press.
+    let mut inputs: Vec<Input> = rx.try_iter().collect();
+    inputs.extend(poll());
+    // Keep the controller adapter nonblocking: gilrs's filtered blocking wait can reset
+    // its timeout indefinitely under stick jitter, starving this channel and Stop.
+    std::thread::sleep(Duration::from_millis(10));
+    inputs
+}
+
 /// Apply one input's actions, delegating hardware recording to the daemon.
 pub fn apply_actions(herdr: &Herdr<impl Runner>, actions: Vec<Action>, mut record: impl FnMut(Action)) {
     for action in actions {
--- a/src/serve_tests.rs
+++ b/src/serve_tests.rs
@@ -72,3 +72,38 @@
     apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
     assert_eq!(fake.calls().last().map(String::as_str), Some("herdr agent send-keys w5:p1 down"));
 }
+
+#[test]
+fn completed_transcript_does_not_need_a_controller_event() {
+    let (tx, rx) = std::sync::mpsc::channel();
+    tx.send(Input::Transcript("ready".into())).unwrap();
+    let mut polled = 0;
+    let inputs = next_inputs(|| { polled += 1; None }, &rx);
+    assert_eq!(inputs, vec![Input::Transcript("ready".into())]);
+    assert_eq!(polled, 1);
+    // Paired with the real gilrs timing probe and channel tests above. The closure passed
+    // to next_inputs must not hide gilrs's unbounded filtered blocking wait.
+    let source = include_str!("serve.rs");
+    let daemon = source.split("/// Apply one input").next().unwrap();
+    assert!(daemon.contains("gilrs.next_event()"), "use gilrs nonblocking poll");
+    assert!(!daemon.contains("next_event_blocking"));
+}
+
+#[test]
+fn completed_transcript_precedes_a_new_recording_press() {
+    let (tx, rx) = std::sync::mpsc::channel();
+    tx.send(Input::Transcript("ready".into())).unwrap();
+    assert_eq!(next_inputs(|| Some(Input::Press(Btn::RStick)), &rx),
+        vec![Input::Transcript("ready".into()), Input::Press(Btn::RStick)]);
+}
+
+#[test]
+fn an_idle_tick_is_throttled_and_poll_is_called_once() {
+    let (_tx, rx) = std::sync::mpsc::channel();
+    let mut calls = 0;
+    let start = std::time::Instant::now();
+    let inputs = next_inputs(|| { calls += 1; None }, &rx);
+    assert_eq!(calls, 1);
+    assert!(start.elapsed() >= std::time::Duration::from_millis(8), "idle polling must not spin");
+    assert!(inputs.is_empty());
+}
```

Run `cargo test --locked`. Scratch final result: 122 unit + 4 integration tests pass (126 total on the release-automation base). After integrating close controls the total will be larger.

Run `cargo mutants --file src/serve.rs --re next_inputs --output /tmp/voice-poll-mutants -- --offline`. Scratch result: 1 caught, 1 unviable (`Input` has no Default), zero survivors. No existing pure business function changed.

Manually remove each scheduling step and run `cargo test --locked --bin herd-gamepad serve_tests`: throttle, transcript drain, controller poll, correct ordering, and nonblocking API. Scratch results: all five were caught by running assertion failures, then source restored.

Commit fix as `fix: poll controllers without delaying completed dictation`.

## Task 3: Review and install

Run spec and quality reviews and the required different-model review. Build the development plugin through its supported build path, install atomically, restart using Start/Stop actions after checking no recording is active, and have the user hold/release voice without touching another button. Do not auto-confirm any agent dialog. Keep this behavior separate from the close menu in commits and PR notes.

## Verified scratch artifacts

- `/tmp/voice-wait-fix`: assembled final crate, production files, three tests, and these docs.
- `/tmp/voice-wait-probe/fix.patch`: exact final patch.
- `/tmp/voice-wait-probe/src/main.rs`: real-library old/new differential.
- `/tmp/voice-wait-probe/production_probe.rs`: hardware probe using the actual new helper (temporary probe binary excluded from final source).
- `/tmp/voice-wait-mutants`: cargo-mutants output.

No application source in the real repository was changed during plan verification.
