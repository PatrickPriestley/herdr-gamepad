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
        let _ = herdr.apply(action);
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
        let _ = (core, herdr); Ok(())
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
        let _ = herdr; Ok(core.handle(input, snap))
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
fn render(dir: &Path) -> Result<(), String> {
        let _ = dir; Err("not implemented".into())
    }
#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;
