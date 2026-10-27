//! Current on-demand close display and bounded error notifications; neither keeps a queue.
use crate::{
    engine::*,
    herdr::{Herdr, Runner},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::{fs::DirBuilderExt, io::AsRawFd},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub struct Feedback {
    last: Option<Instant>,
}
impl Feedback {
    pub fn emit(&mut self, herdr: &Herdr<impl Runner>, action: &Action) {
        let Action::Toast { title, body, sound } = action else {
            return;
        };
        crate::logging::event(format_args!("herd-gamepad: {title}: {body}"));
        if !self.admit(*sound, Instant::now()) {
            return;
        }
        if let Err(e) = herdr.apply(action) {
            crate::logging::event(format_args!("herd-gamepad: notification: {e}"));
        }
    }
    fn admit(&mut self, sound: Sound, now: Instant) -> bool {
        if sound != Sound::Request
            || self
                .last
                .is_some_and(|t| now.saturating_duration_since(t) < Duration::from_secs(5))
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

pub struct MenuUi {
    root: PathBuf,
    active: Option<Session>,
}
struct Session {
    dir: PathBuf,
    text: String,
    revision: u64,
    started: Instant,
    seen: bool,
}
impl MenuUi {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.into(),
            active: None,
        }
    }
    pub fn sync(&mut self, core: &mut Core, herdr: &Herdr<impl Runner>) -> Result<(), String> {
        let result = self.sync_inner(core, herdr);
        if result.is_err() {
            core.cancel_close();
            self.active = None;
        }
        result
    }
    fn sync_inner(&mut self, core: &mut Core, herdr: &Herdr<impl Runner>) -> Result<(), String> {
        let Some(text) = core.close_view() else {
            self.active = None;
            return Ok(());
        };
        if let Some(session) = &mut self.active {
            let alive = session.alive();
            if !alive && (session.seen || session.started.elapsed() >= Duration::from_secs(2)) {
                if !session.seen {
                    return Err("close menu did not start".into());
                }
                core.cancel_close();
                self.active = None;
                return Ok(());
            }
            session.seen |= alive;
            if session.text != text {
                session.text = text;
                session.revision += 1;
                session.write()?;
            }
        } else {
            let session = Session::new(&self.root, text)?;
            let env = format!("GAMEPAD_MENU_DIR={}", session.dir.display());
            herdr.runner.run(&[
                "herdr",
                "plugin",
                "pane",
                "open",
                "--plugin",
                crate::config::PLUGIN_ID,
                "--entrypoint",
                "close-menu",
                "--no-focus",
                "--env",
                &env,
            ])?;
            self.active = Some(session);
        }
        Ok(())
    }
    pub fn handle(
        &mut self,
        core: &mut Core,
        input: Input,
        snap: &Snapshot,
        herdr: &Herdr<impl Runner>,
    ) -> Result<Vec<Action>, String> {
        let was_menu = core.close_view().is_some();
        self.sync(core, herdr)?;
        // Consume an input aimed at a renderer that disappeared, rather than forwarding it to an agent.
        if was_menu && core.close_view().is_none() {
            return Ok(vec![]);
        }
        if input == Input::Press(Btn::A)
            && was_menu
            && !self.active.as_ref().is_some_and(Session::ready)
        {
            return Ok(vec![]);
        }
        let mut actions = core.handle(input, snap);
        let is_menu = core.close_view().is_some();
        self.sync(core, herdr)?;
        if was_menu || is_menu {
            actions.retain(|a| !matches!(a, Action::Toast { .. }));
        }
        Ok(actions)
    }
}
impl Session {
    fn new(root: &Path, text: String) -> Result<Self, String> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = root.join(format!(
            "close-menu-{}-{stamp}-{sequence}",
            std::process::id()
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|e| e.to_string())?;
        let session = Self {
            dir,
            text,
            revision: 1,
            started: Instant::now(),
            seen: false,
        };
        File::create(session.dir.join("viewer.lock")).map_err(|e| e.to_string())?;
        session.write()?;
        Ok(session)
    }
    fn write(&self) -> Result<(), String> {
        atomic(
            &self.dir,
            "state",
            &serde_json::json!({"revision":self.revision,"text":self.text}).to_string(),
        )
    }
    fn alive(&self) -> bool {
        let Ok(f) = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.dir.join("viewer.lock"))
        else {
            return false;
        };
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return false;
        }
        io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK)
    }
    fn ready(&self) -> bool {
        self.alive()
            && fs::read_to_string(self.dir.join("ack")).ok().as_deref()
                == Some(&self.revision.to_string())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn atomic(dir: &Path, name: &str, text: &str) -> Result<(), String> {
    let temporary = dir.join(format!("{name}.next"));
    fs::write(&temporary, text)
        .and_then(|_| fs::rename(temporary, dir.join(name)))
        .map_err(|e| e.to_string())
}
pub fn render_from_env() -> Result<(), String> {
    let dir = std::env::var_os("GAMEPAD_MENU_DIR")
        .ok_or("close menu must be opened by the controller")?;
    render(Path::new(&dir))
}
fn acquire_lease(fd: std::os::fd::RawFd) -> Result<(), String> {
    // The daemon briefly takes this same lock to probe renderer ownership. Give
    // that probe time to release it, while still rejecting a second live viewer.
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EWOULDBLOCK) {
            return Err(error.to_string());
        }
        if Instant::now() >= deadline {
            return Err("close menu already displayed".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn render(dir: &Path) -> Result<(), String> {
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("viewer.lock"))
        .map_err(|e| e.to_string())?;
    acquire_lease(lease.as_raw_fd())?;
    let mut shown = 0;
    loop {
        let state = match fs::read_to_string(dir.join("state")) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        let value: serde_json::Value = serde_json::from_str(&state).map_err(|e| e.to_string())?;
        let revision = value["revision"].as_u64().ok_or("invalid menu revision")?;
        let text = value["text"].as_str().ok_or("invalid menu text")?;
        if revision != shown {
            let mut out = io::stdout().lock();
            write!(out, "\x1b[2J\x1b[H{text}\r\n")
                .and_then(|_| out.flush())
                .map_err(|e| e.to_string())?;
            atomic(dir, "ack", &revision.to_string())?;
            shown = revision;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;
