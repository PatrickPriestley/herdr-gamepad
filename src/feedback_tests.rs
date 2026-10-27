use super::*;
use std::{
    cell::RefCell,
    process::{Child, Command, Stdio},
};
#[derive(Default)]
struct Fake {
    calls: RefCell<Vec<Vec<String>>>,
    fail: bool,
}
impl Runner for Fake {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        if self.fail {
            Err("popup unavailable".into())
        } else {
            Ok("{}".into())
        }
    }
}
fn root() -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "gamepad-feedback-test-{}-{seq}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&p).unwrap();
    p
}
struct Viewer(Child);
impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn viewer(dir: &Path) -> Viewer {
    Viewer(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "feedback::tests::renderer_fixture",
                "--nocapture",
            ])
            .env("GAMEPAD_MENU_TEST_DIR", dir)
            .env("GAMEPAD_MENU_DIR", dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn wait_ack(dir: &Path, revision: u64) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while fs::read_to_string(dir.join("ack")).ok().as_deref() != Some(&revision.to_string()) {
        assert!(
            Instant::now() < deadline,
            "renderer must acknowledge revision {revision}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn renderer_fixture() {
    if let Some(dir) = std::env::var_os("GAMEPAD_MENU_TEST_DIR") {
        let _ = dir;
        render_from_env().unwrap();
    }
}
#[test]
fn routine_feedback_is_quiet_and_errors_do_not_queue() {
    let r = Fake::default();
    let h = Herdr { runner: &r };
    let mut f = Feedback::default();
    f.emit(&h, &toast("error", "one", Sound::Request));
    assert_eq!(r.calls.borrow().len(), 1);
    f.emit(&h, &toast("Navigate mode", "", Sound::None));
    f.emit(&h, &toast("error", "two", Sound::Request));
    assert_eq!(
        r.calls.borrow().len(),
        1,
        "routine and repeated errors must not queue"
    );
    f.last = Some(Instant::now() - Duration::from_secs(5));
    f.emit(&h, &toast("error", "three", Sound::Request));
    assert_eq!(r.calls.borrow().len(), 2);
}
#[test]
fn menu_requires_the_current_live_renderer_and_never_toasts() {
    let root = root();
    let r = Fake::default();
    let h = Herdr { runner: &r };
    let mut ui = MenuUi::new(&root);
    let mut c = Core::new(Btn::RStick);
    let s = crate::engine_tests::snap();
    let out = ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
    assert!(
        ui.active.is_some(),
        "explicit Menu must launch current display"
    );
    assert!(out.is_empty());
    let dir = ui.active.as_ref().unwrap().dir.clone();
    let v = viewer(&dir);
    wait_ack(&dir, 1);
    ui.sync(&mut c, &h).unwrap();
    // Prevent a renderer race while checking the stale acknowledgement gate.
    unsafe {
        libc::kill(v.0.id() as i32, libc::SIGSTOP);
    }
    let out = ui.handle(&mut c, Input::Press(Btn::X), &s, &h).unwrap();
    assert!(out.is_empty());
    assert_eq!(ui.active.as_ref().unwrap().revision, 2);
    assert!(ui
        .active
        .as_ref()
        .unwrap()
        .text
        .contains("Close pane w5:p1?"));
    assert!(ui
        .handle(&mut c, Input::Press(Btn::A), &s, &h)
        .unwrap()
        .is_empty());
    assert!(c.close_view().is_some());
    unsafe {
        libc::kill(v.0.id() as i32, libc::SIGCONT);
    }
    wait_ack(&dir, 2);
    assert_eq!(
        ui.handle(&mut c, Input::Press(Btn::A), &s, &h).unwrap(),
        vec![Action::ClosePane("w5:p1".into())]
    );
    assert!(ui.active.is_none());
    assert!(!dir.exists());
    // A second popup instance cannot reuse the first one's acknowledgement.
    ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
    let dir2 = ui.active.as_ref().unwrap().dir.clone();
    assert_ne!(dir, dir2);
    let v2 = viewer(&dir2);
    wait_ack(&dir2, 1);
    ui.handle(&mut c, Input::Press(Btn::Y), &s, &h).unwrap();
    wait_ack(&dir2, 2);
    drop(v2);
    assert!(ui
        .handle(&mut c, Input::Press(Btn::A), &s, &h)
        .unwrap()
        .is_empty());
    assert!(c.close_view().is_none());
    assert!(ui.active.is_none());
    assert!(r
        .calls
        .borrow()
        .iter()
        .all(|a| a.get(1).map(String::as_str) == Some("plugin")));
    fs::remove_dir(root).unwrap();
}
#[test]
fn popup_failure_disarms_and_close_view_names_the_target() {
    let root = root();
    let r = Fake {
        fail: true,
        ..Fake::default()
    };
    let h = Herdr { runner: &r };
    let mut ui = MenuUi::new(&root);
    let mut c = Core::new(Btn::RStick);
    let s = crate::engine_tests::snap();
    let result = ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h);
    assert!(result.is_err(), "failed launch must surface");
    assert!(c.close_view().is_none());
    assert!(ui.active.is_none());
    c.handle(Input::Press(Btn::Menu), &s);
    assert!(c.close_view().unwrap().contains("w5:p1"));
    c.handle(Input::Press(Btn::Y), &s);
    let text = c.close_view().unwrap();
    assert!(text.contains("Close space w5"));
    assert!(text.contains("Stops its running processes"));
    fs::remove_dir(root).unwrap();
}

fn wait_exit(viewer: &mut Viewer, success: bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = viewer.0.try_wait().unwrap() {
            assert_eq!(status.success(), success, "renderer status: {status}");
            return;
        }
        assert!(Instant::now() < deadline, "renderer must exit");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn startup_timeout_disarms_and_reports_the_missing_display() {
    let root = root();
    let h = Herdr {
        runner: Fake::default(),
    };
    let mut ui = MenuUi::new(&root);
    let mut c = Core::new(Btn::RStick);
    let s = crate::engine_tests::snap();
    ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
    assert!(ui.active.is_some());
    ui.active.as_mut().unwrap().started = Instant::now() - Duration::from_secs(3);
    assert!(
        ui.sync(&mut c, &h).is_err(),
        "accepted popup that never starts must report failure"
    );
    assert!(c.close_view().is_none());
    assert!(ui.active.is_none());
    fs::remove_dir(root).unwrap();
}
#[test]
fn renderer_exit_distinguishes_cancel_from_read_error_and_rejects_duplicates() {
    let root = root();
    let h = Herdr {
        runner: Fake::default(),
    };
    let mut ui = MenuUi::new(&root);
    let mut c = Core::new(Btn::RStick);
    let s = crate::engine_tests::snap();
    ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
    assert!(ui.active.is_some());
    let dir = ui.active.as_ref().unwrap().dir.clone();
    let mut v = viewer(&dir);
    wait_ack(&dir, 1);
    ui.sync(&mut c, &h).unwrap();
    let mut duplicate = viewer(&dir);
    wait_exit(&mut duplicate, false);
    c.cancel_close();
    ui.sync(&mut c, &h).unwrap();
    wait_exit(&mut v, true);
    assert!(!dir.exists());
    ui.handle(&mut c, Input::Press(Btn::Menu), &s, &h).unwrap();
    let dir = ui.active.as_ref().unwrap().dir.clone();
    let mut v = viewer(&dir);
    wait_ack(&dir, 1);
    ui.sync(&mut c, &h).unwrap();
    fs::remove_file(dir.join("state")).unwrap();
    fs::create_dir(dir.join("state")).unwrap();
    wait_exit(&mut v, false);
    ui.sync(&mut c, &h).unwrap();
    assert!(c.close_view().is_none());
    assert!(ui.active.is_none());
    fs::remove_dir(root).unwrap();
}

#[test]
fn error_gate_has_an_exact_five_second_boundary_and_ignores_routine_messages() {
    let mut f = Feedback::default();
    let now = Instant::now();
    assert!(f.admit(Sound::Request, now));
    assert!(!f.admit(Sound::None, now + Duration::from_secs(10)));
    assert!(!f.admit(Sound::Done, now + Duration::from_secs(10)));
    assert!(!f.admit(
        Sound::Request,
        now + Duration::from_secs(5) - Duration::from_nanos(1)
    ));
    assert!(f.admit(Sound::Request, now + Duration::from_secs(5)));
    assert!(!f.admit(Sound::Request, now + Duration::from_secs(5)));
}

#[test]
fn ordinary_actions_bypass_the_popup_and_a_dead_lease_cannot_authorize_close() {
    let root = root();
    let r = Fake::default();
    let h = Herdr { runner: &r };
    let mut ui = MenuUi::new(&root);
    let mut core = Core::new(Btn::RStick);
    let snap = crate::engine_tests::snap();
    assert_eq!(
        ui.handle(&mut core, Input::Press(Btn::X), &snap, &h)
            .unwrap(),
        vec![Action::ToggleZoom("w5:p1".into())]
    );
    assert!(r.calls.borrow().is_empty());
    ui.handle(&mut core, Input::Press(Btn::Menu), &snap, &h)
        .unwrap();
    let session = ui.active.as_ref().unwrap();
    let v = viewer(&session.dir);
    wait_ack(&session.dir, 1);
    assert!(session.ready());
    drop(v);
    assert!(
        !session.ready(),
        "a persisted ack is not proof of a live renderer"
    );
    drop(ui);
    fs::remove_dir(root).unwrap();
}
#[test]
fn close_view_uses_the_configured_cancel_button() {
    let mut core = Core::new(Btn::Menu);
    let snap = crate::engine_tests::snap();
    core.handle(Input::Press(Btn::LStick), &snap);
    assert!(core
        .close_view()
        .unwrap()
        .contains("Right face / Back / left stick click: cancel"));
}
