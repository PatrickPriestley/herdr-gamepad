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

struct Session {
    dir: PathBuf,
    text: String,
    revision: u64,
    started: Instant,
    seen: bool,
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
fn write_frame(out: &mut impl Write, text: &str) -> io::Result<()> {
    write!(out, "\x1b[2J\x1b[H{text}").and_then(|_| out.flush())
}
fn render(dir: &Path) -> Result<(), String> {
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("viewer.lock"))
        .map_err(|e| e.to_string())?;
    acquire_lease(lease.as_raw_fd())?;
    let _cursor = Cursor;
    print!("\x1b[?25l");
    let mut shown = 0;
    let mut last_size = (0, 0);
    loop {
        let state = match fs::read_to_string(dir.join("state")) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        let value: serde_json::Value = serde_json::from_str(&state).map_err(|e| e.to_string())?;
        let revision = value["revision"].as_u64().ok_or("invalid menu revision")?;
        let size = terminal_size();
        let text = if let Some(view) = value.get("view") {
            let view: crate::menu::View =
                serde_json::from_value(view.clone()).map_err(|e| e.to_string())?;
            crate::menu_render::render(&view, size.0, size.1)
        } else {
            value["text"]
                .as_str()
                .ok_or("invalid menu text")?
                .to_string()
        };
        if revision != shown || size != last_size {
            let mut out = io::stdout().lock();
            write_frame(&mut out, &text).map_err(|e| e.to_string())?;
            atomic(dir, "ack", &revision.to_string())?;
            atomic(dir, "size", &size.0.to_string())?;
            last_size = size;
            shown = revision;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;

struct Cursor;
impl Drop for Cursor {
    fn drop(&mut self) {
        print!("\x1b[?25h");
        let _ = io::stdout().flush();
    }
}
fn terminal_size() -> (usize, usize) {
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } == 0
        && size.ws_col > 0
        && size.ws_row > 0
    {
        (size.ws_col as usize, size.ws_row as usize)
    } else {
        (80, 24)
    }
}
/// Passive publisher. Opening the external popup belongs to the cancellable read worker.
pub struct Publisher {
    session: Session,
}
impl Publisher {
    pub fn new(root: &Path, view: &crate::menu::View) -> Result<Self, String> {
        let mut publisher = Self {
            session: Session::new(root, String::new())?,
        };
        publisher.publish(view)?;
        Ok(publisher)
    }
    pub fn directory(&self) -> &Path {
        &self.session.dir
    }
    pub fn publish(&mut self, view: &crate::menu::View) -> Result<(), String> {
        self.session.revision = view.revision;
        atomic(
            &self.session.dir,
            "state",
            &serde_json::json!({"revision":view.revision,"view":view}).to_string(),
        )
    }
    pub fn poll(&mut self) -> Result<(u64, Option<usize>), String> {
        let alive = self.session.alive();
        if !alive && (self.session.seen || self.session.started.elapsed() > Duration::from_secs(5))
        {
            return Err("Actions menu viewer closed".into());
        }
        self.session.seen |= alive;
        let ack = if self.session.ready() {
            self.session.revision
        } else {
            0
        };
        let width = fs::read_to_string(self.session.dir.join("size"))
            .ok()
            .and_then(|s| s.parse().ok());
        Ok((ack, width))
    }
}
