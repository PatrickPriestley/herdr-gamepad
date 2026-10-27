//! Bounded persistent diagnostics, independent of Herdr's action output capture.
use std::{fs::{self, OpenOptions}, io::{Read, Seek, SeekFrom, Write}, path::{Path, PathBuf}, sync::{Mutex, OnceLock}};
pub struct RollingLog { state: PathBuf, limit: usize }
impl RollingLog {
    pub fn new(state: &Path, limit: usize) -> Result<Self, String> {
        fs::create_dir_all(state).map_err(|e| e.to_string())?;
        OpenOptions::new().create(true).append(true).open(state.join("daemon.log")).map_err(|e| e.to_string())?;
        Ok(Self { state: state.into(), limit })
    }
    pub fn append(&mut self, message: &str) -> Result<(), String> {
        let mut end = message.len().min(self.limit.saturating_sub(1));
        while !message.is_char_boundary(end) { end -= 1; }
        let line = format!("{}\n", &message[..end]);
        let path = self.state.join("daemon.log");
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() + line.len() as u64 > self.limit as u64 {
            fs::rename(&path, self.state.join("daemon.log.1")).map_err(|e| e.to_string())?;
        }
        OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| f.write_all(line.as_bytes())).map_err(|e| e.to_string())
    }
}
static LOG: OnceLock<Mutex<RollingLog>> = OnceLock::new();
pub fn init(state: &Path) -> Result<(), String> {
    let log = RollingLog::new(state, 1024 * 1024)?;
    LOG.set(Mutex::new(log)).map_err(|_| "logger already initialized".to_string())
}
pub fn event(message: std::fmt::Arguments<'_>) {
    if let Some(log) = LOG.get() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        if let Err(e) = log.lock().unwrap().append(&format!("{now} {message}")) { eprintln!("log write failed: {e}"); }
    } else { eprintln!("{message}"); }
}
pub fn tail(state: &Path) -> Result<String, String> {
    let mut out = Vec::new();
    for name in ["daemon.log.1", "daemon.log"] {
        match std::fs::File::open(state.join(name)) {
            Ok(mut f) => {
                let len = f.metadata().map_err(|e| e.to_string())?.len();
                f.seek(SeekFrom::Start(len.saturating_sub(32768))).map_err(|e| e.to_string())?;
                f.take(32768).read_to_end(&mut out).map_err(|e| e.to_string())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(if out.is_empty() { "No daemon logs yet.\n".into() } else { String::from_utf8_lossy(&out).into_owned() })
}
