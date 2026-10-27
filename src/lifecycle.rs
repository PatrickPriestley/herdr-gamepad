//! Instance-scoped stop requests; the flock, never a PID, owns daemon lifetime.
use std::{fs::{self, File, OpenOptions}, io::Write, os::unix::io::AsRawFd, path::{Path, PathBuf}, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

#[derive(Clone)]
pub struct Cancellation { request: PathBuf, flag: Arc<AtomicBool> }
impl Cancellation {
    pub fn cancelled(&self) -> bool { self.flag.load(Ordering::SeqCst) || self.request.exists() }
    pub fn cancel(&self) { self.flag.store(true, Ordering::SeqCst); }
}
pub struct Daemon { _lock: File, cancellation: Cancellation }
impl Daemon {
    pub fn acquire(state: &Path) -> Result<Option<Self>, String> {
        let _control = control(state)?;
        let Some(mut lock) = lock(state)? else { return Ok(None); };
        let token = format!("v1-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos());
        lock.set_len(0).and_then(|_| write!(lock, "{token}")).and_then(|_| lock.flush()).map_err(|e| e.to_string())?;
        Ok(Some(Self { _lock: lock, cancellation: Cancellation { request: state.join(format!("daemon.stop.{token}")), flag: Arc::new(AtomicBool::new(false)) } }))
    }
    pub fn cancellation(&self) -> Cancellation { self.cancellation.clone() }
}
impl Drop for Daemon {
    fn drop(&mut self) { self.cancellation.cancel(); let _ = fs::remove_file(&self.cancellation.request); }
}
fn lock(state: &Path) -> Result<Option<File>, String> {
    fs::create_dir_all(state).map_err(|e| e.to_string())?;
    let f = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(state.join("daemon.pid")).map_err(|e| e.to_string())?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 { return Ok(Some(f)); }
    let e = std::io::Error::last_os_error();
    if e.raw_os_error() == Some(libc::EWOULDBLOCK) { Ok(None) } else { Err(e.to_string()) }
}
fn control(state: &Path) -> Result<File, String> {
    fs::create_dir_all(state).map_err(|e| e.to_string())?;
    let f = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(state.join("daemon.control")).map_err(|e| e.to_string())?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 { return Err(std::io::Error::last_os_error().to_string()); }
    Ok(f)
}
pub fn stop(state: &Path, timeout: Duration) -> Result<bool, String> {
    let control = control(state)?;
    if lock(state)?.is_some() { return Ok(false); }
    let token = fs::read_to_string(state.join("daemon.pid")).map_err(|e| e.to_string())?;
    if !token.starts_with("v1-") || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("legacy or uninitialized daemon: close the old Gamepad pane and retry".into());
    }
    let request = state.join(format!("daemon.stop.{token}"));
    File::create(&request).map_err(|e| e.to_string())?;
    drop(control); // New starts can proceed while we wait for this instance only.
    let deadline = Instant::now() + timeout;
    loop {
        if lock(state)?.is_some() || fs::read_to_string(state.join("daemon.pid")).map_err(|e| e.to_string())? != token {
            let _ = fs::remove_file(request);
            return Ok(true);
        }
        if Instant::now() >= deadline { return Err("timed out waiting for daemon shutdown".into()); }
        std::thread::sleep(Duration::from_millis(10));
    }
}
