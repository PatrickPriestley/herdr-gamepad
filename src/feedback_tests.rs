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
fn renderer_retries_a_brief_probe_lock_but_rejects_a_persistent_owner() {
    let root = root();
    let session = Session::new(&root, "test menu".into()).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(session.dir.join("viewer.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let mut transient = viewer(&session.dir);
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        transient.0.try_wait().unwrap().is_none(),
        "renderer must wait for a brief probe lock"
    );
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
    wait_ack(&session.dir, 1);
    assert!(
        session.ready(),
        "released probe lock permits the current renderer"
    );
    fs::remove_file(session.dir.join("state")).unwrap();
    wait_exit(&mut transient, true);

    session.write().unwrap();
    fs::remove_file(session.dir.join("ack")).unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let started = Instant::now();
    let mut persistent = viewer(&session.dir);
    wait_exit(&mut persistent, false);
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "ownership gets a short retry window"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "retry ends before daemon startup timeout"
    );
    assert!(
        !session.dir.join("ack").exists(),
        "another lock owner never authorizes this renderer"
    );
    drop(lock);
    drop(session);
    fs::remove_dir(root).unwrap();
}

#[test]
fn lease_acquisition_preserves_non_contention_errors_without_retrying() {
    let root = root();
    let lock = File::create(root.join("lock")).unwrap();
    assert!(acquire_lease(lock.as_raw_fd()).is_ok());
    let started = Instant::now();
    assert_eq!(
        acquire_lease(-1).unwrap_err(),
        io::Error::from_raw_os_error(libc::EBADF).to_string()
    );
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "non-contention errors do not consume the retry window"
    );
    drop(lock);
    fs::remove_file(root.join("lock")).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn passive_viewer_lease_ack_retirement_and_duplicate_guards() {
    let root = root();
    let session = Session::new(&root, "test".into()).unwrap();
    let mut v = viewer(&session.dir);
    wait_ack(&session.dir, 1);
    assert!(session.ready());
    let mut duplicate = viewer(&session.dir);
    wait_exit(&mut duplicate, false);
    let dir = session.dir.clone();
    drop(session);
    wait_exit(&mut v, true);
    assert!(!dir.exists());
    let session = Session::new(&root, "new".into()).unwrap();
    let v = viewer(&session.dir);
    wait_ack(&session.dir, 1);
    assert!(session.ready());
    drop(v);
    assert!(!session.ready());
    drop(session);
    fs::remove_dir(root).unwrap();
}
#[test]
fn renderer_read_errors_are_not_clean_cancellation() {
    let root = root();
    let session = Session::new(&root, "test".into()).unwrap();
    let mut v = viewer(&session.dir);
    wait_ack(&session.dir, 1);
    fs::remove_file(session.dir.join("state")).unwrap();
    fs::create_dir(session.dir.join("state")).unwrap();
    wait_exit(&mut v, false);
    drop(session);
    fs::remove_dir(root).unwrap();
}
fn menu_view() -> crate::menu::View {
    let settings = crate::menu::SettingsSnapshot {
        labels: crate::menu::LabelStyle::Both,
        voice_button: Btn::RStick,
        max_seconds: 60,
        model: "model".into(),
        language: "en".into(),
        config_path: "config".into(),
        version: "v1".into(),
    };
    crate::menu::Menu::new(1, None, settings, "space".into(), "branch".into()).view()
}
#[test]
fn typed_publisher_requires_current_semantic_ack_even_when_clipped_frames_match() {
    let root = root();
    let mut first = menu_view();
    first.scope = format!("{}A", "long".repeat(100));
    let mut second = first.clone();
    second.scope = format!("{}B", "long".repeat(100));
    second.revision += 1;
    assert_eq!(
        crate::menu_render::render(&first, 80, 24),
        crate::menu_render::render(&second, 80, 24)
    );
    let mut publisher = Publisher::new(&root, &first).unwrap();
    let dir = publisher.directory().to_path_buf();
    let v = viewer(&dir);
    wait_ack(&dir, first.revision);
    assert_eq!(publisher.poll().unwrap().0, first.revision);
    unsafe {
        libc::kill(v.0.id() as i32, libc::SIGSTOP);
    }
    publisher.publish(&second).unwrap();
    assert_eq!(publisher.poll().unwrap().0, 0);
    unsafe {
        libc::kill(v.0.id() as i32, libc::SIGCONT);
    }
    wait_ack(&dir, second.revision);
    assert_eq!(publisher.poll().unwrap(), (second.revision, Some(80)));
    drop(v);
    assert!(publisher.poll().is_err());
    drop(publisher);
    fs::remove_dir(root).unwrap();
}
#[test]
fn typed_publisher_times_out_without_a_viewer() {
    let root = root();
    let mut publisher = Publisher::new(&root, &menu_view()).unwrap();
    assert_eq!(publisher.poll().unwrap().0, 0);
    publisher.session.started = Instant::now() - Duration::from_secs(6);
    assert!(publisher.poll().is_err());
    drop(publisher);
    fs::remove_dir(root).unwrap();
}
#[test]
fn a_full_height_frame_flushes_without_scroll_inducing_final_newline() {
    struct Output {
        bytes: Vec<u8>,
        flushed: bool,
    }
    impl Write for Output {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushed = true;
            Ok(())
        }
    }
    let frame = crate::menu_render::render(&menu_view(), 80, 8);
    let mut out = Output {
        bytes: vec![],
        flushed: false,
    };
    write_frame(&mut out, &frame).unwrap();
    assert!(out.flushed);
    assert!(out.bytes.starts_with(b"\x1b[2J\x1b[H"));
    assert!(!out.bytes.ends_with(b"\n"));
    assert_eq!(&out.bytes[7..], frame.as_bytes());
}
#[test]
fn typed_viewer_reports_actual_terminal_resize_and_restores_cursor() {
    use std::os::fd::FromRawFd;
    let root = root();
    let publisher = Publisher::new(&root, &menu_view()).unwrap();
    let dir = publisher.directory().to_path_buf();
    let (mut master, mut slave) = (-1, -1);
    let mut size = libc::winsize {
        ws_row: 12,
        ws_col: 41,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        },
        0
    );
    let mut master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "feedback::tests::renderer_fixture",
            "--nocapture",
        ])
        .env("GAMEPAD_MENU_TEST_DIR", &dir)
        .env("GAMEPAD_MENU_DIR", &dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut v = Viewer(child);
    use std::io::Read;
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags, -1);
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    let master_fd = master.as_raw_fd();
    let mut bytes = vec![];
    let mut wait_width = |width: usize| {
        let deadline = Instant::now() + Duration::from_secs(3);
        while fs::read_to_string(dir.join("size")).ok().as_deref() != Some(&width.to_string()) {
            let mut buffer = [0; 4096];
            while let Ok(n) = master.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
            }
            assert!(Instant::now() < deadline, "width {width}");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    wait_width(41);
    let resized = libc::winsize {
        ws_row: 15,
        ws_col: 53,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe { libc::ioctl(master_fd, libc::TIOCSWINSZ, &resized) },
        0
    );
    wait_width(53);
    // A partially initialized terminal size must use the complete fallback.
    for (columns, rows) in [(0, 15), (53, 0)] {
        let incomplete = libc::winsize {
            ws_col: columns,
            ws_row: rows,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe { libc::ioctl(master_fd, libc::TIOCSWINSZ, &incomplete) },
            0
        );
        wait_width(80);
        assert_eq!(
            unsafe { libc::ioctl(master_fd, libc::TIOCSWINSZ, &resized) },
            0
        );
        wait_width(53);
    }
    drop(publisher);
    wait_exit(&mut v, true);
    let mut buffer = [0; 4096];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(_) => break,
        }
    }
    let output = String::from_utf8_lossy(&bytes);
    assert!(output.contains("\x1b[?25l"), "{output:?}");
    assert!(output.contains("\x1b[?25h"));
    assert!(output.matches("\x1b[2J").count() >= 2);
    fs::remove_dir(root).unwrap();
}
