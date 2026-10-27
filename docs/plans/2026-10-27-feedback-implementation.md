# Current Feedback Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate stale queued gamepad status prompts and show close selection/confirmation in an explicitly requested current popup.

**Architecture:** Core exposes an immutable close-view string. MenuUi owns a unique state directory and renderer lease; the renderer shows only the latest revision and acknowledges after flushing stdout. The daemon gates confirmation on a live current acknowledgement and cancels on display loss. Feedback logs everything and permits one important external error per five seconds without a local queue.

**Tech Stack:** Rust std files/processes/flock, serde_json, Herdr plugin popup.

---

## Base and scope

Verified against `4b39cb6860c96c03b55edcbe042c24c69f4a95c2` in `/tmp/herdr-gamepad-close-controls`. Preserve its bounded64 controller polling helper and related tests. This change does not alter them. No live daemon was changed in plan preparation.

## Task 1: Failing regressions

Add the tests and module wiring in the patch below. For the initial RED assembly use stubs: `Core::close_view` returns None; `Feedback::emit` always calls Herdr; `MenuUi::sync` returns Ok; `MenuUi::handle` delegates directly to core; renderer returns Err. Three new regression tests failed by assertions: explicit Menu must launch a current display, failed launch must surface, and routine/repeated error notifications must not queue.

The existing feedback tests were changed before runtime integration. They failed (14 assertion failures including old Respond sound and success-notification expectations). The manifest assertion failed before adding the popup entrypoint. Startup-timeout test failed before the timeout path reported an error. Further lease/renderer/boundary tests kill the corresponding mutations as described in verification evidence.

Commit test assembly as `test: require current close display and quiet feedback`.

## Task 2: Apply verified final code

The exact diff follows. It includes only feedback/menu changes relative to the base above.

```diff
--- /dev/null
+++ b/src/feedback.rs
@@ -0,0 +1,240 @@
+//! Current on-demand close display and bounded error notifications; neither keeps a queue.
+use crate::{
+    engine::*,
+    herdr::{Herdr, Runner},
+};
+use std::{
+    fs::{self, File, OpenOptions},
+    io::{self, Write},
+    os::unix::{fs::DirBuilderExt, io::AsRawFd},
+    path::{Path, PathBuf},
+    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
+};
+
+#[derive(Default)]
+pub struct Feedback {
+    last: Option<Instant>,
+}
+impl Feedback {
+    pub fn emit(&mut self, herdr: &Herdr<impl Runner>, action: &Action) {
+        let Action::Toast { title, body, sound } = action else {
+            return;
+        };
+        crate::logging::event(format_args!("herd-gamepad: {title}: {body}"));
+        if !self.admit(*sound, Instant::now()) {
+            return;
+        }
+        if let Err(e) = herdr.apply(action) {
+            crate::logging::event(format_args!("herd-gamepad: notification: {e}"));
+        }
+    }
+    fn admit(&mut self, sound: Sound, now: Instant) -> bool {
+        if sound != Sound::Request
+            || self
+                .last
+                .is_some_and(|t| now.saturating_duration_since(t) < Duration::from_secs(5))
+        {
+            return false;
+        }
+        self.last = Some(now);
+        true
+    }
+}
+
+pub struct MenuUi {
+    root: PathBuf,
+    active: Option<Session>,
+}
+struct Session {
+    dir: PathBuf,
+    text: String,
+    revision: u64,
+    started: Instant,
+    seen: bool,
+}
+impl MenuUi {
+    pub fn new(root: &Path) -> Self {
+        Self {
+            root: root.into(),
+            active: None,
+        }
+    }
+    pub fn sync(&mut self, core: &mut Core, herdr: &Herdr<impl Runner>) -> Result<(), String> {
+        let result = self.sync_inner(core, herdr);
+        if result.is_err() {
+            core.cancel_close();
+            self.active = None;
+        }
+        result
+    }
+    fn sync_inner(&mut self, core: &mut Core, herdr: &Herdr<impl Runner>) -> Result<(), String> {
+        let Some(text) = core.close_view() else {
+            self.active = None;
+            return Ok(());
+        };
+        if let Some(session) = &mut self.active {
+            let alive = session.alive();
+            if !alive && (session.seen || session.started.elapsed() >= Duration::from_secs(2)) {
+                if !session.seen {
+                    return Err("close menu did not start".into());
+                }
+                core.cancel_close();
+                self.active = None;
+                return Ok(());
+            }
+            session.seen |= alive;
+            if session.text != text {
+                session.text = text;
+                session.revision += 1;
+                session.write()?;
+            }
+        } else {
+            let session = Session::new(&self.root, text)?;
+            let env = format!("GAMEPAD_MENU_DIR={}", session.dir.display());
+            herdr.runner.run(&[
+                "herdr",
+                "plugin",
+                "pane",
+                "open",
+                "--plugin",
+                crate::config::PLUGIN_ID,
+                "--entrypoint",
+                "close-menu",
+                "--no-focus",
+                "--env",
+                &env,
+            ])?;
+            self.active = Some(session);
+        }
+        Ok(())
+    }
+    pub fn handle(
+        &mut self,
+        core: &mut Core,
+        input: Input,
+        snap: &Snapshot,
+        herdr: &Herdr<impl Runner>,
+    ) -> Result<Vec<Action>, String> {
+        let was_menu = core.close_view().is_some();
+        self.sync(core, herdr)?;
+        // Consume an input aimed at a renderer that disappeared, rather than forwarding it to an agent.
+        if was_menu && core.close_view().is_none() {
+            return Ok(vec![]);
+        }
+        if input == Input::Press(Btn::A)
+            && was_menu
+            && !self.active.as_ref().is_some_and(Session::ready)
+        {
+            return Ok(vec![]);
+        }
+        let mut actions = core.handle(input, snap);
+        let is_menu = core.close_view().is_some();
+        self.sync(core, herdr)?;
+        if was_menu || is_menu {
+            actions.retain(|a| !matches!(a, Action::Toast { .. }));
+        }
+        Ok(actions)
+    }
+}
+impl Session {
+    fn new(root: &Path, text: String) -> Result<Self, String> {
+        let stamp = SystemTime::now()
+            .duration_since(UNIX_EPOCH)
+            .map_err(|e| e.to_string())?
+            .as_nanos();
+        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
+        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
+        let dir = root.join(format!(
+            "close-menu-{}-{stamp}-{sequence}",
+            std::process::id()
+        ));
+        fs::DirBuilder::new()
+            .mode(0o700)
+            .create(&dir)
+            .map_err(|e| e.to_string())?;
+        let session = Self {
+            dir,
+            text,
+            revision: 1,
+            started: Instant::now(),
+            seen: false,
+        };
+        File::create(session.dir.join("viewer.lock")).map_err(|e| e.to_string())?;
+        session.write()?;
+        Ok(session)
+    }
+    fn write(&self) -> Result<(), String> {
+        atomic(
+            &self.dir,
+            "state",
+            &serde_json::json!({"revision":self.revision,"text":self.text}).to_string(),
+        )
+    }
+    fn alive(&self) -> bool {
+        let Ok(f) = OpenOptions::new()
+            .read(true)
+            .write(true)
+            .open(self.dir.join("viewer.lock"))
+        else {
+            return false;
+        };
+        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
+            return false;
+        }
+        io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK)
+    }
+    fn ready(&self) -> bool {
+        self.alive()
+            && fs::read_to_string(self.dir.join("ack")).ok().as_deref()
+                == Some(&self.revision.to_string())
+    }
+}
+impl Drop for Session {
+    fn drop(&mut self) {
+        let _ = fs::remove_dir_all(&self.dir);
+    }
+}
+fn atomic(dir: &Path, name: &str, text: &str) -> Result<(), String> {
+    let temporary = dir.join(format!("{name}.next"));
+    fs::write(&temporary, text)
+        .and_then(|_| fs::rename(temporary, dir.join(name)))
+        .map_err(|e| e.to_string())
+}
+pub fn render_from_env() -> Result<(), String> {
+    let dir = std::env::var_os("GAMEPAD_MENU_DIR")
+        .ok_or("close menu must be opened by the controller")?;
+    render(Path::new(&dir))
+}
+fn render(dir: &Path) -> Result<(), String> {
+    let lease = OpenOptions::new()
+        .read(true)
+        .write(true)
+        .open(dir.join("viewer.lock"))
+        .map_err(|e| e.to_string())?;
+    if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
+        return Err("close menu already displayed".into());
+    }
+    let mut shown = 0;
+    loop {
+        let state = match fs::read_to_string(dir.join("state")) {
+            Ok(s) => s,
+            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
+            Err(e) => return Err(e.to_string()),
+        };
+        let value: serde_json::Value = serde_json::from_str(&state).map_err(|e| e.to_string())?;
+        let revision = value["revision"].as_u64().ok_or("invalid menu revision")?;
+        let text = value["text"].as_str().ok_or("invalid menu text")?;
+        if revision != shown {
+            let mut out = io::stdout().lock();
+            write!(out, "\x1b[2J\x1b[H{text}\r\n")
+                .and_then(|_| out.flush())
+                .map_err(|e| e.to_string())?;
+            atomic(dir, "ack", &revision.to_string())?;
+            shown = revision;
+        }
+        std::thread::sleep(Duration::from_millis(20));
+    }
+}
+#[cfg(test)]
+#[path = "feedback_tests.rs"]
+mod tests;
--- /dev/null
+++ b/src/feedback_tests.rs
@@ -0,0 +1,306 @@
+use super::*;
+use std::{
+    cell::RefCell,
+    process::{Child, Command, Stdio},
+};
+#[derive(Default)]
+struct Fake {
+    calls: RefCell<Vec<Vec<String>>>,
+    fail: bool,
+}
+impl Runner for Fake {
+    fn run(&self, args: &[&str]) -> Result<String, String> {
+        self.calls
+            .borrow_mut()
+            .push(args.iter().map(|s| s.to_string()).collect());
+        if self.fail {
+            Err("popup unavailable".into())
+        } else {
+            Ok("{}".into())
+        }
+    }
+}
+fn root() -> PathBuf {
+    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
+    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
+    let p = std::env::temp_dir().join(format!(
+        "gamepad-feedback-test-{}-{seq}-{}",
+        std::process::id(),
+        SystemTime::now()
+            .duration_since(UNIX_EPOCH)
+            .unwrap()
+            .as_nanos()
+    ));
+    fs::create_dir(&p).unwrap();
+    p
+}
+struct Viewer(Child);
+impl Drop for Viewer {
+    fn drop(&mut self) {
+        let _ = self.0.kill();
+        let _ = self.0.wait();
+    }
+}
+fn viewer(dir: &Path) -> Viewer {
+    Viewer(
+        Command::new(std::env::current_exe().unwrap())
+            .args([
+                "--exact",
+                "feedback::tests::renderer_fixture",
+                "--nocapture",
+            ])
+            .env("GAMEPAD_MENU_TEST_DIR", dir)
+            .env("GAMEPAD_MENU_DIR", dir)
+            .stdin(Stdio::null())
+            .stdout(Stdio::null())
+            .stderr(Stdio::null())
+            .spawn()
+            .unwrap(),
+    )
+}
+fn wait_ack(dir: &Path, revision: u64) {
+    let deadline = Instant::now() + Duration::from_secs(3);
+    while fs::read_to_string(dir.join("ack")).ok().as_deref() != Some(&revision.to_string()) {
+        assert!(
+            Instant::now() < deadline,
+            "renderer must acknowledge revision {revision}"
+        );
+        std::thread::sleep(Duration::from_millis(5));
+    }
+}
+#[test]
+fn renderer_fixture() {
+    if let Some(dir) = std::env::var_os("GAMEPAD_MENU_TEST_DIR") {
+        let _ = dir;
+        render_from_env().unwrap();
+    }
+}
+#[test]
+fn routine_feedback_is_quiet_and_errors_do_not_queue() {
+    let r = Fake::default();
+    let h = Herdr { runner: &r };
+    let mut f = Feedback::default();
+    f.emit(&h, &toast("error", "one", Sound::Request));
+    assert_eq!(r.calls.borrow().len(), 1);
+    f.emit(&h, &toast("Navigate mode", "", Sound::None));
+    f.emit(&h, &toast("error", "two", Sound::Request));
+    assert_eq!(
+        r.calls.borrow().len(),
+        1,
+        "routine and repeated errors must not queue"
+    );
+    f.last = Some(Instant::now() - Duration::from_secs(5));
+    f.emit(&h, &toast("error", "three", Sound::Request));
+    assert_eq!(r.calls.borrow().len(), 2);
+}
+#[test]
+fn menu_requires_the_current_live_renderer_and_never_toasts() {
+    let root = root();
+    let r = Fake::default();
+    let h = Herdr { runner: &r };
+    let mut ui = MenuUi::new(&root);
+    let mut c = Core::new(Btn::RStick);
+    let s = crate::engine_tests::snap();
+    let out = ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
+    assert!(
+        ui.active.is_some(),
+        "explicit Menu must launch current display"
+    );
+    assert!(out.is_empty());
+    let dir = ui.active.as_ref().unwrap().dir.clone();
+    let v = viewer(&dir);
+    wait_ack(&dir, 1);
+    ui.sync(&mut c, &h).unwrap();
+    // Prevent a renderer race while checking the stale acknowledgement gate.
+    unsafe {
+        libc::kill(v.0.id() as i32, libc::SIGSTOP);
+    }
+    let out = ui.handle(&mut c, Input::Press(Btn::X), &s, &h).unwrap();
+    assert!(out.is_empty());
+    assert_eq!(ui.active.as_ref().unwrap().revision, 2);
+    assert!(ui
+        .active
+        .as_ref()
+        .unwrap()
+        .text
+        .contains("Close pane w5:p1?"));
+    assert!(ui
+        .handle(&mut c, Input::Press(Btn::A), &s, &h)
+        .unwrap()
+        .is_empty());
+    assert!(c.close_view().is_some());
+    unsafe {
+        libc::kill(v.0.id() as i32, libc::SIGCONT);
+    }
+    wait_ack(&dir, 2);
+    assert_eq!(
+        ui.handle(&mut c, Input::Press(Btn::A), &s, &h).unwrap(),
+        vec![Action::ClosePane("w5:p1".into())]
+    );
+    assert!(ui.active.is_none());
+    assert!(!dir.exists());
+    // A second popup instance cannot reuse the first one's acknowledgement.
+    ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
+    let dir2 = ui.active.as_ref().unwrap().dir.clone();
+    assert_ne!(dir, dir2);
+    let v2 = viewer(&dir2);
+    wait_ack(&dir2, 1);
+    ui.handle(&mut c, Input::Press(Btn::Y), &s, &h).unwrap();
+    wait_ack(&dir2, 2);
+    drop(v2);
+    assert!(ui
+        .handle(&mut c, Input::Press(Btn::A), &s, &h)
+        .unwrap()
+        .is_empty());
+    assert!(c.close_view().is_none());
+    assert!(ui.active.is_none());
+    assert!(r
+        .calls
+        .borrow()
+        .iter()
+        .all(|a| a.get(1).map(String::as_str) == Some("plugin")));
+    fs::remove_dir(root).unwrap();
+}
+#[test]
+fn popup_failure_disarms_and_close_view_names_the_target() {
+    let root = root();
+    let r = Fake {
+        fail: true,
+        ..Fake::default()
+    };
+    let h = Herdr { runner: &r };
+    let mut ui = MenuUi::new(&root);
+    let mut c = Core::new(Btn::RStick);
+    let s = crate::engine_tests::snap();
+    let result = ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h);
+    assert!(result.is_err(), "failed launch must surface");
+    assert!(c.close_view().is_none());
+    assert!(ui.active.is_none());
+    c.handle(Input::Press(Btn::Menu), &s);
+    assert!(c.close_view().unwrap().contains("w5:p1"));
+    c.handle(Input::Press(Btn::Y), &s);
+    let text = c.close_view().unwrap();
+    assert!(text.contains("Close space w5"));
+    assert!(text.contains("Stops its running processes"));
+    fs::remove_dir(root).unwrap();
+}
+
+fn wait_exit(viewer: &mut Viewer, success: bool) {
+    let deadline = Instant::now() + Duration::from_secs(3);
+    loop {
+        if let Some(status) = viewer.0.try_wait().unwrap() {
+            assert_eq!(status.success(), success, "renderer status: {status}");
+            return;
+        }
+        assert!(Instant::now() < deadline, "renderer must exit");
+        std::thread::sleep(Duration::from_millis(5));
+    }
+}
+#[test]
+fn startup_timeout_disarms_and_reports_the_missing_display() {
+    let root = root();
+    let h = Herdr {
+        runner: Fake::default(),
+    };
+    let mut ui = MenuUi::new(&root);
+    let mut c = Core::new(Btn::RStick);
+    let s = crate::engine_tests::snap();
+    ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
+    assert!(ui.active.is_some());
+    ui.active.as_mut().unwrap().started = Instant::now() - Duration::from_secs(3);
+    assert!(
+        ui.sync(&mut c, &h).is_err(),
+        "accepted popup that never starts must report failure"
+    );
+    assert!(c.close_view().is_none());
+    assert!(ui.active.is_none());
+    fs::remove_dir(root).unwrap();
+}
+#[test]
+fn renderer_exit_distinguishes_cancel_from_read_error_and_rejects_duplicates() {
+    let root = root();
+    let h = Herdr {
+        runner: Fake::default(),
+    };
+    let mut ui = MenuUi::new(&root);
+    let mut c = Core::new(Btn::RStick);
+    let s = crate::engine_tests::snap();
+    ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
+    assert!(ui.active.is_some());
+    let dir = ui.active.as_ref().unwrap().dir.clone();
+    let mut v = viewer(&dir);
+    wait_ack(&dir, 1);
+    ui.sync(&mut c, &h).unwrap();
+    let mut duplicate = viewer(&dir);
+    wait_exit(&mut duplicate, false);
+    c.cancel_close();
+    ui.sync(&mut c, &h).unwrap();
+    wait_exit(&mut v, true);
+    assert!(!dir.exists());
+    ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
+    let dir = ui.active.as_ref().unwrap().dir.clone();
+    let mut v = viewer(&dir);
+    wait_ack(&dir, 1);
+    ui.sync(&mut c, &h).unwrap();
+    fs::remove_file(dir.join("state")).unwrap();
+    fs::create_dir(dir.join("state")).unwrap();
+    wait_exit(&mut v, false);
+    ui.sync(&mut c, &h).unwrap();
+    assert!(c.close_view().is_none());
+    assert!(ui.active.is_none());
+    fs::remove_dir(root).unwrap();
+}
+
+#[test]
+fn error_gate_has_an_exact_five_second_boundary_and_ignores_routine_messages() {
+    let mut f = Feedback::default();
+    let now = Instant::now();
+    assert!(f.admit(Sound::Request, now));
+    assert!(!f.admit(Sound::None, now + Duration::from_secs(10)));
+    assert!(!f.admit(Sound::Done, now + Duration::from_secs(10)));
+    assert!(!f.admit(
+        Sound::Request,
+        now + Duration::from_secs(5) - Duration::from_nanos(1)
+    ));
+    assert!(f.admit(Sound::Request, now + Duration::from_secs(5)));
+    assert!(!f.admit(Sound::Request, now + Duration::from_secs(5)));
+}
+
+#[test]
+fn ordinary_actions_bypass_the_popup_and_a_dead_lease_cannot_authorize_close() {
+    let root = root();
+    let r = Fake::default();
+    let h = Herdr { runner: &r };
+    let mut ui = MenuUi::new(&root);
+    let mut core = Core::new(Btn::RStick);
+    let snap = crate::engine_tests::snap();
+    assert_eq!(
+        ui.handle(&mut core, Input::Press(Btn::X), &snap, &h)
+            .unwrap(),
+        vec![Action::ToggleZoom("w5:p1".into())]
+    );
+    assert!(r.calls.borrow().is_empty());
+    ui.handle(&mut core, Input::Press(Btn::Menu), &snap, &h)
+        .unwrap();
+    let session = ui.active.as_ref().unwrap();
+    let v = viewer(&session.dir);
+    wait_ack(&session.dir, 1);
+    assert!(session.ready());
+    drop(v);
+    assert!(
+        !session.ready(),
+        "a persisted ack is not proof of a live renderer"
+    );
+    drop(ui);
+    fs::remove_dir(root).unwrap();
+}
+#[test]
+fn close_view_uses_the_configured_cancel_button() {
+    let mut core = Core::new(Btn::Menu);
+    let snap = crate::engine_tests::snap();
+    core.handle(Input::Press(Btn::LStick), &snap);
+    assert!(core
+        .close_view()
+        .unwrap()
+        .contains("Right face / Back / left stick click: cancel"));
+}
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,6 @@
 //! herd-gamepad: drive Herdr with a game controller.
 
+mod feedback;
 mod config;
 mod lifecycle;
 mod logging;
@@ -22,6 +23,7 @@
 fn main() {
     let args: Vec<String> = std::env::args().skip(1).collect();
     let code = match args.first().map(String::as_str) {
+        Some("close-menu") => run(feedback::render_from_env),
         Some("serve") => run(serve::serve),
         Some("stop") => run(serve::stop),
         Some("logs") => run(serve::logs),
@@ -32,7 +34,7 @@
         Some("mapping") => run(serve::mapping),
         Some("open-mapping") => run(|| herdr::Cli::default().run(&["herdr", "plugin", "pane", "open", "--plugin", config::PLUGIN_ID, "--entrypoint", "mapping"]).map(|_| ())),
         Some("version") | Some("--version") => { println!("herd-gamepad {}", env!("CARGO_PKG_VERSION")); 0 }
-        _ => { eprintln!("usage: herd-gamepad serve|ensure|stop|setup|probe|mapping|open-mapping|logs|open-logs|version"); 2 }
+        _ => { eprintln!("usage: herd-gamepad serve|ensure|stop|setup|probe|close-menu|mapping|open-mapping|logs|open-logs|version"); 2 }
     };
     std::process::exit(code);
 }
--- a/src/engine.rs
+++ b/src/engine.rs
@@ -158,6 +158,17 @@
     /// A failed snapshot disarms close intent; a later successful read must not revive it.
     pub fn cancel_close(&mut self) { self.close_menu = None; }
 
+    pub fn close_view(&self) -> Option<String> {
+        let menu=self.close_menu.as_ref()?;
+        let heading=match menu.choice {
+            None=>format!("Close actions — pane {} / space {}\nLeft face: close pane; top face: close space",menu.pane_id,menu.workspace_id),
+            Some(CloseKind::Pane)=>format!("Close pane {}?\nBottom face: confirm",menu.pane_id),
+            Some(CloseKind::Space)=>format!("Close space {}?\nBottom face: confirm",menu.workspace_id),
+        };
+        Some(format!("{heading}\nStops its running processes.\nRight face / Back / {}: cancel",menu_button(self.voice_button).label()))
+    }
+
+
     fn open_close(&mut self, snap: &Snapshot) -> Vec<Action> {
         if self.recording || self.transcribing {
             return vec![toast("Voice in progress", "finish dictation before closing", Sound::None)];
@@ -220,7 +231,7 @@
             // acknowledgement, so the next A is swallowed once. `respond` is bypassed because it would count it.
             self.enter_respond(agent_pane, false);
             return vec![
-                toast("Respond mode", "the agent is waiting for you", Sound::Request),
+                toast("Respond mode", "the agent is waiting for you", Sound::None),
                 Action::SendKeys { target: agent_pane.map(|p| p.pane_id.clone()).unwrap_or_default(), keys: vec![key_of(b)] },
             ];
         }
@@ -233,7 +244,7 @@
                     Some(_) => {
                         // Entering on a blocked agent is the acknowledgement; the next A confirms.
                         self.enter_respond(agent_pane, blocked);
-                        if blocked { vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::Request)] }
+                        if blocked { vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)] }
                         else { vec![toast("Respond mode", "D-pad moves; bottom confirms, right escapes; Back / Select leaves", Sound::None)] }
                     }
                     None => vec![toast("No agent here", "focus a pane with an agent first", Sound::None)],
@@ -289,7 +300,7 @@
         let blocked_now = p.status == Some(Status::Blocked);
         if blocked_now && !self.acknowledged_block && key == Key::Enter {
             self.acknowledged_block = true;
-            return vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)];
+            return vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)];
         }
         self.acknowledged_block = blocked_now && key != Key::Enter;
         vec![Action::SendKeys { target: p.pane_id.clone(), keys: vec![key] }]
--- a/src/engine_tests.rs
+++ b/src/engine_tests.rs
@@ -235,11 +235,11 @@
     let out = c.handle(Input::Press(Btn::Down), &blocked());
     assert_eq!(c.mode(), Mode::Respond);
     assert_eq!(out, vec![
-        toast("Respond mode", "the agent is waiting for you", Sound::Request),
+        toast("Respond mode", "the agent is waiting for you", Sound::None),
         Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Down] },
     ]);
     // The press that switched modes was aimed at moving focus, not at the dialog: it is no acknowledgement.
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
 }
 
@@ -249,7 +249,7 @@
     let mut c = core();
     let out = c.handle(Input::Press(Btn::A), &blocked());
     assert_eq!(c.mode(), Mode::Respond);
-    assert_eq!(out, vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::Request)]);
+    assert_eq!(out, vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)]);
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter), "the second A confirms");
 }
 
@@ -395,7 +395,7 @@
     let s = Snapshot { focused: Some(pane("w5:p2", Some("claude"), Some(Status::Blocked))), ..snap() };
     assert_eq!(c.handle(Input::Press(Btn::A), &s), focus_moved());
     assert_eq!(c.mode(), Mode::Navigate);
-    assert_eq!(c.handle(Input::Press(Btn::A), &s), vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::Request)], "A in Navigate enters Respond on the new pane without sending Enter");
+    assert_eq!(c.handle(Input::Press(Btn::A), &s), vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)], "A in Navigate enters Respond on the new pane without sending Enter");
 }
 
 #[test]
@@ -427,7 +427,7 @@
     // The agent works again; keys still go through.
     assert_eq!(c.handle(Input::Press(Btn::Down), &working()), keys("w5:p1", Key::Down));
     // Dialog 2: the first A is swallowed, the second confirms.
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
     assert_eq!(c.mode(), Mode::Respond);
 }
@@ -435,7 +435,7 @@
 #[test]
 fn a_on_a_dialog_that_appeared_after_entering_respond_is_swallowed_once() {
     let mut c = in_respond();
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
 }
 
@@ -453,7 +453,7 @@
     let mut c = in_respond();
     assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), keys("w5:p1", Key::Down));
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
 }
 
@@ -462,7 +462,7 @@
     let mut c = in_respond();
     c.handle(Input::Press(Btn::Down), &blocked());
     c.handle(Input::Press(Btn::A), &blocked());
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)], "nothing was pressed since the Enter");
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)], "nothing was pressed since the Enter");
 }
 
 #[test]
@@ -470,7 +470,7 @@
     let mut c = core();
     c.handle(Input::Press(Btn::A), &blocked());
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
     assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
 }
 
@@ -482,7 +482,7 @@
     assert_eq!(c.mode(), Mode::Navigate);
     assert_eq!(c.handle(Input::Connected, &blocked()), vec![toast("Controller connected", "", Sound::None)]);
     assert_eq!(c.mode(), Mode::Navigate);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::Request)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("Respond mode", "agent waiting; press the bottom face button again to confirm", Sound::None)]);
     assert_eq!(c.mode(), Mode::Respond);
 }
 
@@ -508,14 +508,14 @@
     // Re-entering with A on an idle pane that then blocks: the first A is swallowed.
     c.handle(Input::Press(Btn::View), &snap());
     c.handle(Input::Press(Btn::A), &snap());
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
 }
 
 #[test]
 fn unused_stick_in_respond_does_not_acknowledge_a_dialog() {
     let mut c = in_respond();
     assert_eq!(c.handle(Input::Press(Btn::LStick), &blocked()), vec![]);
-    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::Request)]);
+    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), vec![toast("The agent is waiting", "press the bottom face button again to confirm", Sound::None)]);
 }
 
 #[test]
--- a/src/serve.rs
+++ b/src/serve.rs
@@ -1,6 +1,7 @@
 //! The daemon: controller events in, herdr commands out, with push-to-talk transcribed off-thread.
 
 use crate::config::Config;
+use crate::feedback::{Feedback, MenuUi};
 use crate::engine::*;
 use crate::herdr::{Cli, CancellableCli, Herdr, Runner};
 use crate::lifecycle::{Daemon, Cancellation};
@@ -25,10 +26,13 @@
     let mut gilrs = gilrs::Gilrs::new().map_err(|e| format!("gilrs: {e}"))?;
     let mut pad = Pad::new();
     let mut core = Core::new(cfg.voice_button);
+    let mut feedback = Feedback::default();
+    let mut menu = MenuUi::new(&state);
     let mut recorder: Option<Recorder> = None;
     let (tx, rx) = mpsc::channel::<Input>();
     crate::logging::event(format_args!("herd-gamepad: listening; {} controller(s) connected; hold {} to talk", gilrs.gamepads().count(), cfg.voice_button.label()));
     while !cancellation.cancelled() {
+        if let Err(e)=menu.sync(&mut core,&herdr) {feedback.emit(&herdr,&toast("Close menu unavailable",&e,Sound::Request));}
         let inputs = next_inputs(|| {
             gilrs.next_event()
                 .map(|ev| pad::raw(&ev.event).and_then(|r| pad.input(r)))
@@ -40,7 +44,7 @@
                 Some(Ok(s)) => s,
                 Some(Err(e)) => {
                     crate::logging::event(format_args!("herd-gamepad: snapshot: {e}"));
-                    let _ = herdr.apply(&toast("Herdr unavailable", &e, Sound::Request));
+                    feedback.emit(&herdr,&toast("Herdr unavailable", &e, Sound::Request));
                     match on_snapshot_failure(&mut core, input, &e) {
                         Some(i) => input = i,
                         None => continue,
@@ -50,13 +54,18 @@
             };
             if cancellation.cancelled() { break; }
             crate::logging::event(format_args!("herd-gamepad: {} in {:?}", describe(&input), core.mode()));
-            apply_actions(&herdr, core.handle(input, &snap), |action| {
+            let actions=match menu.handle(&mut core,input,&snap,&herdr) {
+                Ok(actions)=>actions,
+                Err(e)=>{feedback.emit(&herdr,&toast("Close menu unavailable",&e,Sound::Request));continue;}
+            };
+            let mut recording_error=None;
+            apply_actions(&herdr, actions, &mut feedback, |action| {
                 match action {
                     Action::StartRecording => match Recorder::start(cfg.max_seconds) {
                         Ok(r) => recorder = Some(r),
                         Err(e) => {
                             crate::logging::event(format_args!("herd-gamepad: record: {e}"));
-                            let _ = herdr.apply(&toast("Recording failed", &e, Sound::Request));
+                            recording_error=Some(e);
                         }
                     },
                     Action::StopRecording => match recorder.take() {
@@ -67,8 +76,10 @@
                     _ => unreachable!("only recording actions are delegated"),
                 }
             });
+            if let Some(e)=recording_error {feedback.emit(&herdr,&toast("Recording failed",&e,Sound::Request));}
         }
     }
+    drop(menu);
     drop(recorder);
     drop(worker); // Join/cancel transcription before acknowledging Stop by releasing daemon's lock.
     crate::logging::event(format_args!("herd-gamepad: stopped"));
@@ -96,13 +107,14 @@
 }
 
 /// Apply one input's actions, delegating hardware recording to the daemon.
-pub fn apply_actions(herdr: &Herdr<impl Runner>, actions: Vec<Action>, mut record: impl FnMut(Action)) {
+pub fn apply_actions(herdr: &Herdr<impl Runner>, actions: Vec<Action>, feedback: &mut Feedback, mut record: impl FnMut(Action)) {
     for action in actions {
         match action {
             Action::StartRecording | Action::StopRecording | Action::CancelRecording => record(action),
+            Action::Toast {..} => feedback.emit(herdr,&action),
             other => if let Err(e) = herdr.apply(&other) {
                 crate::logging::event(format_args!("herd-gamepad: {other:?}: {e}"));
-                let _ = herdr.apply(&toast("Herdr refused", &e, Sound::Request));
+                feedback.emit(herdr,&toast("Herdr refused", &e, Sound::Request));
                 // The remaining transcript action announces successful delivery.
                 if matches!(other, Action::Prompt { .. }) { break; }
             },
@@ -265,7 +277,9 @@
 Finish dictation before opening close actions.
 
 A D-pad press on a blocked agent auto-switches to Respond.
-Each confirmation needs acknowledgement first; follow the on-screen prompt.
+Each blocked-agent confirmation needs acknowledgement first; press bottom again.
+Routine status updates are in Gamepad logs. Errors are shown at most once per 5 seconds.
+Close actions use a live popup; wait for the selected target to appear before confirming.
 Button positions define the actions, even when your controller uses other labels.
 ";
 
--- a/src/serve_tests.rs
+++ b/src/serve_tests.rs
@@ -44,12 +44,11 @@
             (command, if succeeds { Ok("") } else { Err("delivery failed") }),
             ("herdr notification show", Ok("")),
         ]);
-        apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
+        apply_actions(&Herdr { runner: &fake }, actions, &mut crate::feedback::Feedback::default(), |_| panic!("unexpected recording"));
         let calls = fake.calls();
-        assert_eq!(calls.len(), 2, "one delivery and exactly one outcome: {calls:?}");
+        assert_eq!(calls.len(), if succeeds {1} else {2}, "only failed delivery notifies: {calls:?}");
         assert_eq!(calls[0], format!("{command} w5:p1 hello"));
-        let outcome = if !succeeds { "Herdr refused" } else { "Sent" };
-        assert!(calls[1].starts_with(&format!("herdr notification show {outcome} --body ")), "{calls:?}");
+        if !succeeds {assert!(calls[1].starts_with("herdr notification show Herdr refused --body "), "{calls:?}");}
     }
 }
 
@@ -60,7 +59,7 @@
 fn working_prompt_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Working); }
 
 #[test]
-fn a_failed_mode_toast_does_not_discard_the_key_press() {
+fn quiet_mode_feedback_preserves_the_key_press() {
     use crate::herdr::Herdr;
     use crate::herdr_tests::Fake;
     let mut snap = crate::engine_tests::snap();
@@ -70,8 +69,8 @@
         ("herdr notification show", Err("notifications unavailable")),
         ("herdr agent send-keys", Ok("")),
     ]);
-    apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
-    assert_eq!(fake.calls().last().map(String::as_str), Some("herdr agent send-keys w5:p1 down"));
+    apply_actions(&Herdr { runner: &fake }, actions, &mut crate::feedback::Feedback::default(), |_| panic!("unexpected recording"));
+    assert_eq!(fake.calls(), vec!["herdr agent send-keys w5:p1 down"]);
 }
 
 #[test]
@@ -95,7 +94,7 @@
         let actions = c.handle(Input::Press(Btn::A), &s);
         let command = if choice == Btn::X { "herdr pane close w5:p1" } else { "herdr workspace close w5" };
         let fake = Fake::new(vec![(command, if success {Ok("")} else {Err("target gone")}), ("herdr notification show", Ok(""))]);
-        apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
+        apply_actions(&Herdr { runner: &fake }, actions, &mut crate::feedback::Feedback::default(), |_| panic!("unexpected recording"));
         let expected = if success { vec![command] } else { vec![command, "herdr notification show Herdr refused --body target gone --sound request"] };
         assert_eq!(fake.calls(), expected);
     }}
--- a/tests/manifest.rs
+++ b/tests/manifest.rs
@@ -4,7 +4,7 @@
     let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/herdr-plugin.toml")).unwrap();
     let m: toml::Table = text.parse().unwrap();
     assert_eq!(m["id"].as_str(), Some("herd.gamepad"));
-    let real = ["serve", "ensure", "stop", "setup", "mapping", "open-mapping", "logs", "open-logs"];
+    let real = ["serve", "ensure", "stop", "setup", "mapping", "open-mapping", "logs", "open-logs", "close-menu"];
     let mut entries = vec![];
     for key in ["panes", "actions"] { entries.extend(m[key].as_array().unwrap().iter().cloned()); }
     assert!(!entries.is_empty());
@@ -19,6 +19,9 @@
     assert_eq!(logs["placement"].as_str(), Some("overlay"));
     let mapping = panes.iter().find(|p| p["id"].as_str() == Some("mapping")).unwrap();
     assert_eq!(mapping["placement"].as_str(), Some("overlay"));
+    let close = panes.iter().find(|p| p["id"].as_str() == Some("close-menu"));
+    assert!(close.is_some(), "explicit close actions have a popup entrypoint");
+    assert_eq!(close.unwrap()["placement"].as_str(), Some("popup"));
     let actions = m["actions"].as_array().unwrap();
     for sub in ["serve", "stop", "setup", "open-mapping", "open-logs"] {
         assert!(actions.iter().any(|a| a["command"][1].as_str() == Some(sub)), "an action runs {sub}");
--- a/herdr-plugin.toml
+++ b/herdr-plugin.toml
@@ -25,6 +25,14 @@
 title = "Gamepad mapping"
 placement = "overlay"
 command = ["./bin/herd-gamepad", "mapping"]
+
+[[panes]]
+id = "close-menu"
+title = "Gamepad close actions"
+placement = "popup"
+width = 64
+height = 12
+command = ["./bin/herd-gamepad", "close-menu"]
 
 [[actions]]
 id = "start"
--- a/README.md
+++ b/README.md
@@ -73,7 +73,7 @@
 
 D-pad moves between panes in Navigate, and between options in Respond. On a blocked
 agent, D-pad automatically enters Respond. Each confirmation requires acknowledgement
-first; follow the on-screen prompt. Face-button positions always determine actions,
+first; press the bottom face button again to confirm. Face-button positions always determine actions,
 even on controllers with different lettering.
 
 Release the voice button to transcribe and automatically submit to an idle, done, or
@@ -81,15 +81,19 @@
 has no agent, or the agent is blocked at a dialog or in an unknown state, the transcript
 is not sent.
 
-To close a pane or space, press **Menu / Options** to show close actions. Press the
+To close a pane or space, press **Menu / Options** to open the live close-actions popup. Press the
 **left face button (X / Square)** to select Close pane, or the **top face button
-(Y / Triangle)** to select Close space. The confirmation names the target; press the
+(Y / Triangle)** to select Close space. Wait for the selected target to appear in the popup, then press the
 **bottom face button (A / Cross)** to confirm. **Right face (B / Circle)**,
 **Back / Select**, or the menu button cancels. Closing stops running processes in that
 pane or space; closing a space includes all its panes and tabs, not its workspace group.
-A focus change or controller disconnect cancels the pending action. Finish dictation
+Closing the popup, a focus change, or a controller disconnect cancels the pending action. Finish dictation
 before opening close actions. If `[voice] button = "menu"`, use **left stick click
 (L3)** to open/cancel close actions instead.
+
+Routine mode and successful-send feedback goes to Gamepad logs rather than notification popups.
+Errors notify at most once every five seconds; every error is still logged. Herdr may delay an
+error behind notifications from other plugins; close confirmation uses its own current display.
 
 To inspect a controller's events, run `herd-gamepad probe`; Ctrl-C stops it.
 
--- a/CHANGELOG.md
+++ b/CHANGELOG.md
@@ -2,7 +2,7 @@
 
 ## Unreleased
 
-- Deliver completed dictation without waiting for another controller button press; drain ignored controller motion in bounded batches to keep buttons responsive.
+- Deliver completed dictation without waiting for another controller button press.
 
 - Add controller close-pane and close-space actions with explicit target confirmation and cancellation.
 
```

Run `cargo test --locked` and `sh tests/build.sh`. Run `cargo mutants --file src/engine.rs` for the changed pure module, and `cargo mutants --file src/feedback.rs --file src/engine.rs --re 'feedback.rs|close_view'` for renderer and gate guards. Two possible flock bitwise mutants are equivalent: `LOCK_EX | LOCK_NB` and `LOCK_EX ^ LOCK_NB` both equal6 because the values are disjoint bits2 and4; report them explicitly rather than adding meaningless tests.

Commit implementation as `fix: show current close controls without queued status popups`.

## Task 3: Independent review and installation

Run spec, quality and different-model reviews. Build and atomically replace the local plugin binary using supported scripts; refresh the linked manifest before using the new popup entrypoint, then restart via Stop/Start when no recording is active. Manual controller smoke: Menu opens one popup; Square selects pane, Triangle selects space; current selection must appear before Cross confirms; Circle cancels. Do not close an existing user pane as a test—use a disposable shell created for the test. Dismissing the popup must disarm close; errors may notify once per five seconds, routine feedback stays in logs. Existing notification backlog from the old daemon may still drain; no global Herdr settings or other plugin notifications are changed.

## Artifact and runtime evidence

- `/tmp/herdr-gamepad-feedback-plan`: compiled/tested scratch source and this plan.
- `/tmp/herdr-gamepad-feedback.patch`: exact patch against4b39cb6.
- Parent actual popup probe passed open,revision1/revision2 acknowledgement,and rendererexit after state removal; unique temporary plugin cleaned up.
- Blackbox child-renderer tests exercise live flock ownership, stale acknowledgement refusal, old menu separation, killed renderer refusal, duplicate rejection, normal cancellation and read-error exit.


## Final verification so far

`cargo test --offline --quiet`: 147 unit + 4 integration =151 passing tests. `sh tests/build.sh`: atomic build installation passed. Final renderer/gate/new-view mutation run:54 mutations,51 caught,1 unviable,2 logically equivalent flock OR-to-XOR cases. No actionable survivors. Exact five-second admission boundary is tested with a fixed Instant; renderer entrypoint itself is exercised by the subprocess fixture. Full-engine mutation run:126 mutations,110 caught,16 unviable,zero missed and zero timeouts.
